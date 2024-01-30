use anyhow::{Context, Error};
use clap::Parser;
use futures::future::join_all;
use std::{
    io::{stderr, stdout, Write},
    path::{Path, PathBuf},
};
use tokio::{fs as tfs, process::Command, sync::mpsc, task::JoinHandle};

#[derive(Debug)]
struct VCSInfo {
    name: &'static str,
    dir: &'static str,
    cmd: &'static [&'static str],
}

const VCS_INFO: &[VCSInfo; 3] = &[
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

#[derive(Parser)]
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

    let args = Args::parse();

    let (tx, mut rx) = mpsc::channel::<(PathBuf, &'static VCSInfo)>(8);
    let receiver = if args.count {
        tokio::spawn(async move {
            let mut count = 0;
            while let Some((path, VCSInfo { cmd, .. })) = rx.recv().await {
                let output = Command::new(cmd[0])
                    .args(&cmd[1..])
                    .current_dir(&path)
                    .output()
                    .await?;

                if output.stdout.len() + output.stderr.len() > 0 {
                    count += 1;
                }
            }

            println!("{count}");

            Ok::<(), Error>(())
        })
    } else {
        tokio::spawn(async move {
            while let Some((path, VCSInfo { name, cmd, .. })) = rx.recv().await {
                let output = Command::new(cmd[0])
                    .args(&cmd[1..])
                    .current_dir(&path)
                    .output()
                    .await
                    .with_context(|| "exec failed")?;

                if output.stdout.len() + output.stderr.len() > 0 {
                    let mut stdout = stdout();
                    let path = path.to_string_lossy();
                    println!("{path} - {name}");
                    stdout.write_all(&output.stdout)?;
                    stderr().write_all(&output.stderr)?;
                };
            }

            Ok::<(), Error>(())
        })
    };

    let senders = args.paths.into_iter().map(|path| {
        let tx = tx.clone();
        tokio::spawn(async move { traverse(tx, &path.canonicalize()?).await })
    });
    join_all_handles(senders).await?;
    drop(tx);

    receiver.await??;

    Ok(())
}

#[async_recursion::async_recursion]
async fn traverse(tx: mpsc::Sender<(PathBuf, &'static VCSInfo)>, path: &Path) -> Result<(), Error> {
    for vcs_info @ VCSInfo { dir, .. } in VCS_INFO {
        if tfs::try_exists(path.join(dir))
            .await
            .with_context(|| "stat failed")?
        {
            tx.send((path.to_owned(), &vcs_info))
                .await
                .expect("channel shouldn't have been closed");
            return Ok(());
        }
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

        let tx = tx.clone();
        handles.push(tokio::spawn(
            async move { traverse(tx, &entry.path()).await },
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
