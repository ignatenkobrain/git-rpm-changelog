extern crate chrono;
extern crate failure;
extern crate git2;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;
extern crate tempdir;

use std::path::PathBuf;
use std::process::{self, Command};

use chrono::{FixedOffset, TimeZone};
use failure::Error;
use git2::Repository;
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

    let spec = format!(
        "{}.spec",
        repo.workdir()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
    );
    for id in walker {
        let oid = id?;

        let worktree = TempDir::new("git-rpm-changelog")?;
        let workrepo = Repository::open(&opt.path)?;
        workrepo.set_workdir(worktree.path(), false)?;
        let object = workrepo.find_object(oid, Some(git2::ObjectType::Commit))?;
        workrepo.checkout_tree(
            &object,
            Some(&mut git2::build::CheckoutBuilder::new()
                .force()
                .update_index(false)),
        )?;
        let workdir = workrepo.workdir().unwrap();

        let commit = object.as_commit().unwrap();
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

        println!("{}\n{}\n", chlog_header, chlog_entry);
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
