extern crate chrono;
extern crate failure;
extern crate git2;
extern crate rayon;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;
extern crate tempdir;

use std::path::PathBuf;
use std::process::{self, Command};

use chrono::{FixedOffset, TimeZone};
use failure::Error;
use git2::Repository;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use structopt::StructOpt;
use tempdir::TempDir;

#[derive(StructOpt, Debug)]
#[structopt(name = "git-rpm-changelog")]
struct Opt {
    #[structopt(help = "Path to git repository", parse(from_os_str))]
    path: PathBuf,
}

fn run(opt: &Opt) -> Result<(), Error> {
    let repo = Repository::open(&opt.path)?;
    let mut walker = repo.revwalk()?;

    walker.set_sorting(git2::SORT_TOPOLOGICAL);
    walker.push_head()?;

    let walker = walker.collect::<Result<Vec<_>, _>>()?;

    let spec = format!(
        "{}.spec",
        repo.workdir()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
    );
    let changelog = walker
        .into_par_iter()
        .map(|oid| {
            let workrepo = Repository::open(&opt.path)?;
            let commit = workrepo.find_commit(oid)?;
            let tree = commit.tree()?;
            let entry = tree.get_name(&spec);
            if entry.is_none() {
                return Ok(None);
            }

            let worktree = TempDir::new("git-rpm-changelog")?;
            workrepo.set_workdir(worktree.path(), false)?;
            workrepo.checkout_tree(
                commit.as_object(),
                Some(&mut git2::build::CheckoutBuilder::new()
                    .force()
                    .update_index(false)),
            )?;
            let workdir = workrepo.workdir().unwrap();

            let author = commit.author();
            let atime = author.when();
            let datetime =
                FixedOffset::east(atime.offset_minutes() * 3600 / 60).timestamp(atime.seconds(), 0);
            let output = Command::new("rpmspec")
                .args(&[
                    "--srpm",
                    "--query",
                    "--queryformat",
                    "%|EPOCH?{%{EPOCH}:}|%{VERSION}-%{RELEASE}",
                    "--undefine",
                    "dist",
                    "--define",
                    format!("_sourcedir {}", workdir.to_str().unwrap()).as_str(),
                    workdir.join(&spec).to_str().unwrap(),
                ])
                .output()?;

            let mut chlog_header = format!(
                "* {} {} <{}>",
                datetime.format("%a %b %d %T %z %Y"),
                author.name().unwrap_or("Nobody"),
                author.email().unwrap_or("nobody@fedoraproject.org"),
            );
            if output.status.success() {
                chlog_header.push_str(&format!(" - {}", String::from_utf8_lossy(&output.stdout)));
            } else {
                eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            }
            let chlog_entry = format!("- {}", commit.summary().unwrap());

            Ok(Some(format!("{}\n{}", chlog_header, chlog_entry)))
        })
        .filter(|c| c.as_ref().map(|c| c.is_some()).unwrap_or(true))
        .map(|c| c.map(|c| c.unwrap()))
        .collect::<Result<Vec<_>, Error>>()?;

    for entry in changelog {
        println!("{}\n", entry);
    }

    Ok(())
}

fn main() {
    let opt = Opt::from_args();

    match run(&opt) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    };
}
