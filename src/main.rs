use std::path::{Path, PathBuf};

use anyhow::{Context, Error};
use async_recursion::async_recursion;
use clap::Parser;
use futures::future::join_all;
use tokio::{fs as tfs, process};

const GIT_DIR: &'static str = ".git";
const HG_DIR: &'static str = ".hg";
const SVN_DIR: &'static str = ".svn";

const GIT_CMD: [&str; 5] = ["git", "-c", "color.status=always", "status", "-s"];
const HG_CMD: [&str; 4] = ["hg", "--config", "extensions.color=!", "st"];
const SVN_CMD: [&str; 3] = ["svn", "st", "-v"];

const DIR_CMD_PAIRS: [(&str, &[&str]); 3] =
    [(GIT_DIR, &GIT_CMD), (HG_DIR, &HG_CMD), (SVN_DIR, &SVN_CMD)];

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    // TODO
    // #[clap(short = 'n', help = "Display number of modified repositories")]
    // count: bool,
    #[clap(default_value = ".")]
    paths: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let handles = Args::parse()
        .paths
        .into_iter()
        .map(|path| tokio::spawn(async move { traverse(&path).await }));
    if let Some(e) =
        join_all(handles)
            .await
            .into_iter()
            .find_map(|r| match r.with_context(|| "join failed") {
                Err(e) | Ok(Err(e)) => Some(e),
                _ => None,
            })
    {
        Err(e)
    } else {
        Ok(())
    }
}

#[async_recursion]
async fn traverse(path: &Path) -> Result<(), Error> {
    for (dir, cmd) in DIR_CMD_PAIRS {
        if tfs::try_exists(path.join(dir))
            .await
            .with_context(|| "stat failed")?
        {
            return process::Command::new(cmd[0])
                .args(&cmd[1..])
                .current_dir(&path)
                .status()
                .await
                .map(|_| ())
                .with_context(|| "exec failed");
        };
    }

    let mut dir_entries = tfs::read_dir(path)
        .await
        .with_context(|| "readdir failed")?;

    let mut handles = Vec::new();
    while let Some(entry) = dir_entries
        .next_entry()
        .await
        .with_context(|| "readdir next failed")?
    {
        if !entry
            .metadata()
            .await
            .with_context(|| "metadata failed")?
            .is_dir()
        {
            continue;
        }

        handles.push(tokio::spawn(async move { traverse(&entry.path()).await }));
    }

    if let Some(e) =
        join_all(handles)
            .await
            .into_iter()
            .find_map(|r| match r.with_context(|| "join failed") {
                Err(e) | Ok(Err(e)) => Some(e),
                _ => None,
            })
    {
        Err(e)
    } else {
        Ok(())
    }
}
