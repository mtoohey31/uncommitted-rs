use clap::Parser;
use flume::{self, select::Selector};
use num_cpus;
use std::io::{self, stdout, Write};
use std::process::{exit, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use std::{path::PathBuf, thread};

const GIT_DIR: &'static str = ".git";
const HG_DIR: &'static str = ".hg";
const SVN_DIR: &'static str = ".svn";

const GIT_CMD: [&str; 5] = ["git", "-c", "color.status=always", "status", "-s"];
const HG_CMD: [&str; 4] = ["hg", "--config", "extensions.color=!", "st"];
const SVN_CMD: [&str; 3] = ["svn", "st", "-v"];

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    // #[clap(short = 'n', help = "Display number of modified repositories")]
    // count: bool,
    #[clap(default_value = ".")]
    paths: Vec<PathBuf>,
}

enum Dir {
    GitDir(PathBuf),
    HgDir(PathBuf),
    SvnDir(PathBuf),
}

fn main() {
    let args = Args::parse();
    let (vcds_tx, vcds_rx) = flume::unbounded();
    let (pq_tx, pq_rx) = flume::unbounded();
    let (done_tx, done_rx) = flume::bounded(1);

    for path in args.paths {
        if path.metadata().unwrap().is_dir() {
            pq_tx.send(path).unwrap();
        }
    }

    let num_scanners = (num_cpus::get() - 1).max(1);
    let working = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::with_capacity(num_scanners);
    for _ in 0..num_scanners {
        let vcds_txc = vcds_tx.clone();
        let pq_rxc = pq_rx.clone();
        let pq_txc = pq_tx.clone();
        let done_txc = done_tx.clone();
        let workingc = working.clone();
        threads.push(thread::spawn(move || {
            // TODO: error handling here
            while let Ok(path) = pq_rxc.recv() {
                workingc.fetch_add(1, Ordering::SeqCst);
                if match path.join(GIT_DIR).try_exists() {
                    Ok(v) => v,
                    Err(e) => {
                        done_txc.send(Some(e)).unwrap();
                        return;
                    }
                } {
                    vcds_txc.send(Dir::GitDir(path)).unwrap();
                } else if match path.join(HG_DIR).try_exists() {
                    Ok(v) => v,
                    Err(e) => {
                        done_txc.send(Some(e)).unwrap();
                        return;
                    }
                } {
                    vcds_txc.send(Dir::HgDir(path)).unwrap();
                } else if match path.join(SVN_DIR).try_exists() {
                    Ok(v) => v,
                    Err(e) => {
                        done_txc.send(Some(e)).unwrap();
                        return;
                    }
                } {
                    vcds_txc.send(Dir::SvnDir(path)).unwrap();
                } else {
                    for entry in match path.read_dir() {
                        Ok(v) => v,
                        Err(e) => {
                            done_txc.send(Some(e)).unwrap();
                            return;
                        }
                    } {
                        let entry = match entry {
                            Ok(v) => v,
                            Err(e) => {
                                done_txc.send(Some(e)).unwrap();
                                return;
                            }
                        };
                        if entry.metadata().unwrap().is_dir() {
                            pq_txc.send(entry.path()).unwrap();
                        }
                    }
                }
                if workingc.fetch_sub(1, Ordering::SeqCst) == 1 && pq_txc.is_empty() {
                    done_txc.send(None).unwrap();
                    return;
                };
            }
        }));
    }

    loop {
        if let Err(err) = Selector::new()
            .recv(&vcds_rx, |dir| execute_vc(dir.unwrap()))
            .recv(&done_rx, |merrr| match merrr {
                Ok(merr) => {
                    if let Some(err) = merr {
                        eprintln!("\x1b[31m{}\x1b[0m", err);
                        exit(1);
                    }
                    for gd in vcds_rx.drain() {
                        if let Err(err) = execute_vc(gd) {
                            eprintln!("\x1b[31m{}\x1b[0m", err);
                            exit(1);
                        }
                    }
                    exit(0);
                }
                Err(err) => {
                    eprintln!("\x1b[31m{}\x1b[0m", err);
                    exit(1);
                }
            })
            .wait()
        {
            eprintln!("\x1b[31m{}\x1b[0m", err);
            exit(1);
        }
    }
}

fn execute_vc(path: Dir) -> Result<(), io::Error> {
    match path {
        Dir::GitDir(p) => {
            let mut args = Vec::with_capacity(GIT_CMD.len() - 1);
            GIT_CMD[1..].clone_into(&mut args);
            let output = Command::new(GIT_CMD[0])
                .args(args)
                .current_dir(&p)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output()?
                .stdout;
            if output.len() > 0 {
                println!("\n{} - git", p.display());
                stdout().write(&output).map(|_| ())
            } else {
                Ok(())
            }
        }
        Dir::HgDir(p) => {
            let mut args = Vec::with_capacity(HG_CMD.len() - 1);
            HG_CMD[1..].clone_into(&mut args);
            let output = Command::new(HG_CMD[0])
                .args(args)
                .current_dir(&p)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output()?
                .stdout;
            if output.len() > 0 {
                println!("\n{} - hg", p.display());
                stdout().write(&output).map(|_| ())
            } else {
                Ok(())
            }
        }
        Dir::SvnDir(p) => {
            let mut args = Vec::with_capacity(SVN_CMD.len() - 1);
            SVN_CMD[1..].clone_into(&mut args);
            let output = Command::new(SVN_CMD[0])
                .args(args)
                .current_dir(&p)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output()?
                .stdout;
            if output.len() > 0 {
                println!("\n{} - svn", p.display());
                stdout().write(&output).map(|_| ())
            } else {
                Ok(())
            }
        }
    }
}
