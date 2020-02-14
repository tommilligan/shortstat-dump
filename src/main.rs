/*
 * libgit2 "log" example - shows how to walk history and get commit info
 *
 * Written by the libgit2 contributors
 *
 * To the extent possible under law, the author(s) have dedicated all copyright
 * and related and neighboring rights to this software to the public domain
 * worldwide. This software is distributed without any warranty.
 *
 * You should have received a copy of the CC0 Public Domain Dedication along
 * with this software. If not, see
 * <http://creativecommons.org/publicdomain/zero/1.0/>.
 */

#![deny(warnings)]

use docopt::Docopt;
use git2::{Commit, DiffOptions, ObjectType, Repository};
use git2::{DiffStats, Error, Pathspec};
use serde_derive::{Deserialize, Serialize};
use std::str;

#[derive(Clone, Serialize, Debug, PartialEq)]
pub struct ShortStat {
    #[serde(rename(serialize = "f"))]
    pub files_changed: usize,
    #[serde(rename(serialize = "i"))]
    pub insertions: usize,
    #[serde(rename(serialize = "d"))]
    pub deletions: usize,
}

impl From<DiffStats> for ShortStat {
    fn from(diff_stats: DiffStats) -> Self {
        Self {
            files_changed: diff_stats.files_changed(),
            insertions: diff_stats.insertions(),
            deletions: diff_stats.deletions(),
        }
    }
}

#[derive(Deserialize)]
struct Args {
    arg_commit: Vec<String>,
    arg_spec: Vec<String>,
    flag_topo_order: bool,
    flag_date_order: bool,
    flag_reverse: bool,
    flag_git_dir: Option<String>,
    flag_skip: Option<usize>,
    flag_max_count: Option<usize>,
    flag_merges: bool,
    flag_no_merges: bool,
    flag_no_min_parents: bool,
    flag_no_max_parents: bool,
    flag_max_parents: Option<usize>,
    flag_min_parents: Option<usize>,
    flag_patch: bool,
}

fn run(args: &Args) -> Result<(), Error> {
    let path = args.flag_git_dir.as_ref().map(|s| &s[..]).unwrap_or(".");
    let repo = Repository::open(path)?;
    let mut revwalk = repo.revwalk()?;

    // Prepare the revwalk based on CLI parameters
    let base = if args.flag_reverse {
        git2::Sort::REVERSE
    } else {
        git2::Sort::NONE
    };
    revwalk.set_sorting(
        base | if args.flag_topo_order {
            git2::Sort::TOPOLOGICAL
        } else if args.flag_date_order {
            git2::Sort::TIME
        } else {
            git2::Sort::NONE
        },
    );
    for commit in &args.arg_commit {
        if commit.starts_with('^') {
            let obj = repo.revparse_single(&commit[1..])?;
            revwalk.hide(obj.id())?;
            continue;
        }
        let revspec = repo.revparse(commit)?;
        if revspec.mode().contains(git2::RevparseMode::SINGLE) {
            revwalk.push(revspec.from().unwrap().id())?;
        } else {
            let from = revspec.from().unwrap().id();
            let to = revspec.to().unwrap().id();
            revwalk.push(to)?;
            if revspec.mode().contains(git2::RevparseMode::MERGE_BASE) {
                let base = repo.merge_base(from, to)?;
                let o = repo.find_object(base, Some(ObjectType::Commit))?;
                revwalk.push(o.id())?;
            }
            revwalk.hide(from)?;
        }
    }
    if args.arg_commit.is_empty() {
        revwalk.push_head()?;
    }

    // Prepare our diff options and pathspec matcher
    let (mut diffopts, mut diffopts2) = (DiffOptions::new(), DiffOptions::new());
    for spec in &args.arg_spec {
        diffopts.pathspec(spec);
        diffopts2.pathspec(spec);
    }
    let ps = Pathspec::new(args.arg_spec.iter())?;

    // Filter our revwalk based on the CLI parameters
    macro_rules! filter_try {
        ($e:expr) => {
            match $e {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            }
        };
    }
    let revwalk = revwalk
        .filter_map(|id| {
            let id = filter_try!(id);
            let commit = filter_try!(repo.find_commit(id));
            let parents = commit.parents().len();
            if parents < args.min_parents() {
                return None;
            }
            if let Some(n) = args.max_parents() {
                if parents >= n {
                    return None;
                }
            }
            if !args.arg_spec.is_empty() {
                match commit.parents().len() {
                    0 => {
                        let tree = filter_try!(commit.tree());
                        let flags = git2::PathspecFlags::NO_MATCH_ERROR;
                        if ps.match_tree(&tree, flags).is_err() {
                            return None;
                        }
                    }
                    _ => {
                        let m = commit.parents().all(|parent| {
                            match_with_parent(&repo, &commit, &parent, &mut diffopts)
                                .unwrap_or(false)
                        });
                        if !m {
                            return None;
                        }
                    }
                }
            }
            Some(Ok(commit))
        })
        .skip(args.flag_skip.unwrap_or(0))
        .take(args.flag_max_count.unwrap_or(!0));

    // print!
    for commit in revwalk {
        let commit = commit?;
        if !args.flag_patch || commit.parents().len() > 1 {
            continue;
        }
        let a = if commit.parents().len() == 1 {
            let parent = commit.parent(0)?;
            Some(parent.tree()?)
        } else {
            None
        };
        let b = commit.tree()?;
        let diff = repo.diff_tree_to_tree(a.as_ref(), Some(&b), Some(&mut diffopts2))?;
        let short_stat: ShortStat = diff.stats()?.into();
        println!("{}", serde_json::to_string(&short_stat).unwrap());
    }

    Ok(())
}

fn match_with_parent(
    repo: &Repository,
    commit: &Commit,
    parent: &Commit,
    opts: &mut DiffOptions,
) -> Result<bool, Error> {
    let a = parent.tree()?;
    let b = commit.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&a), Some(&b), Some(opts))?;
    Ok(diff.deltas().len() > 0)
}

impl Args {
    fn min_parents(&self) -> usize {
        if self.flag_no_min_parents {
            return 0;
        }
        self.flag_min_parents
            .unwrap_or(if self.flag_merges { 2 } else { 0 })
    }

    fn max_parents(&self) -> Option<usize> {
        if self.flag_no_max_parents {
            return None;
        }
        self.flag_max_parents
            .or(if self.flag_no_merges { Some(1) } else { None })
    }
}

fn main() {
    const USAGE: &str = "
usage: log [options] [<commit>..] [--] [<spec>..]

Options:
    --topo-order            sort commits in topological order
    --date-order            sort commits in date order
    --reverse               sort commits in reverse
    --author <user>         author to sort by
    --committer <user>      committer to sort by
    --grep <pat>            pattern to filter commit messages by
    --git-dir <dir>         alternative git directory to use
    --skip <n>              number of commits to skip
    -n, --max-count <n>     maximum number of commits to show
    --merges                only show merge commits
    --no-merges             don't show merge commits
    --no-min-parents        don't require a minimum number of parents
    --no-max-parents        don't require a maximum number of parents
    --max-parents <n>       specify a maximum number of parents for a commit
    --min-parents <n>       specify a minimum number of parents for a commit
    -p, --patch             show commit diff
    -h, --help              show this message
";

    let args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());
    match run(&args) {
        Ok(()) => {}
        Err(e) => println!("error: {}", e),
    }
}
