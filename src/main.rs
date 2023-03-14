use std::{
    io::{stderr, stdout, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{Context, Error};
use clap::Parser;
use futures::future::join_all;
use tokio::{fs as tfs, process::Command, task::JoinHandle};

struct VCSInfo<'a> {
    name: &'a str,
    dir: &'a str,
    cmd: &'a [&'a str],
}

const DIR_CMD_PAIRS: [VCSInfo; 3] = [
    VCSInfo {
        name: "git",
        dir: ".git",
        cmd: &["git", "-c", "color.status=always", "status", "-s"],
    },
    VCSInfo {
        name: "mercurial",
        dir: ".hg",
        cmd: &["hg", "--config", "extensions.color=!", "st"],
    },
    VCSInfo {
        name: "subversion",
        dir: ".svn",
        cmd: &["svn", "st", "-v"],
    },
];

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short = 'n', help = "Display number of modified repositories")]
    count: bool,
    #[clap(default_value = ".")]
    paths: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // TODO: figure out how to avoid hitting ulimit by checking it then using a semaphore; may
    // require rewriting tfs::read_dir so we can prevent it from buffering ahead without us
    // actually having aquired the necessary semaphores

    async fn run<M: Mode>(mode: M, paths: Vec<PathBuf>) -> Result<(), Error> {
        let handles = paths
            .into_iter()
            .map(|path| tokio::spawn(async move { traverse(mode, &path.canonicalize()?).await }));
        join_all_handles(handles).await
    }

    let args = Args::parse();
    if args.count {
        let mode = &*Box::leak(Box::new(CountMode::new()));
        run(mode, args.paths).await?;
        println!("{}", mode.0.load(Ordering::Acquire));
    } else {
        run(OutputMode, args.paths).await?;
    }

    Ok(())
}

#[async_trait::async_trait]
trait Mode: Send + Sync + Copy + 'static {
    async fn run(&self, path: &Path, cmd: &[&str], name: &str) -> Result<(), Error>;
}

struct CountMode(AtomicUsize);

impl CountMode {
    fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
}

#[async_trait::async_trait]
impl Mode for &'static CountMode {
    async fn run(&self, path: &Path, cmd: &[&str], _name: &str) -> Result<(), Error> {
        let output = Command::new(cmd[0])
            .args(&cmd[1..])
            .current_dir(&path)
            .output()
            .await?;

        if output.stdout.len() + output.stderr.len() > 0 {
            self.0.fetch_add(1, Ordering::Release);
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
struct OutputMode;

#[async_trait::async_trait]
impl Mode for OutputMode {
    async fn run(&self, path: &Path, cmd: &[&str], name: &str) -> Result<(), Error> {
        let output = Command::new(cmd[0])
            .args(&cmd[1..])
            .current_dir(&path)
            .output()
            .await
            .with_context(|| "exec failed")?;

        if output.stdout.len() + output.stderr.len() > 0 {
            let mut stdout = stdout();
            let path = path.to_string_lossy();
            writeln!(&stdout, "{path} - {name}")?;
            stdout.write_all(&output.stdout)?;
            stderr().write_all(&output.stderr)?;
        };

        Ok(())
    }
}

#[async_recursion::async_recursion]
async fn traverse<M: Mode>(mode: M, path: &Path) -> Result<(), Error> {
    for VCSInfo { name, dir, cmd } in DIR_CMD_PAIRS {
        if tfs::try_exists(path.join(dir))
            .await
            .with_context(|| "stat failed")?
        {
            return mode.run(path, cmd, name).await;
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

        handles.push(tokio::spawn(
            async move { traverse(mode, &entry.path()).await },
        ));
    }

    join_all_handles(handles).await
}

async fn join_all_handles<I>(iter: I) -> Result<(), Error>
where
    I: IntoIterator<Item = JoinHandle<Result<(), Error>>>,
{
    join_all(iter)
        .await
        .into_iter()
        .map(|res| res.with_context(|| "join failed")?)
        .collect()
}
