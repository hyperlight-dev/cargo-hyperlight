use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Make the `dst_dir` into the union of a subset of the directory
/// trees rooted at `src_dirs`. The files considered to be part of the
/// src directory trees are those for which
/// - the pattern `glob` matches their (relative) path; and
/// - the `predicate` returns true
///
/// Bug: extraneous empty directories may remain in the output
/// directory
///
/// For example, with a filesystem layout of:
/// a/
///   b.h
///   no.y
///   skip.h
/// b/
///   c.h
/// c/
///   d.h
///   e.x
/// Calling `union_glob_with_predicate` with `srcs` set to `["a",
/// "b"]` and `dst` to `"c"`, the glob `"*.h"`, and the predicate
/// `|path| path != "skip.h"` would result in `c` containing
/// c/
///   b.h
///   c.h
pub(crate) fn union_glob_with_predicate(
    src_dirs: impl Iterator<Item: AsRef<Path>>,
    dst_dir: &Path,
    glob: &str,
    predicate: impl Fn(&Path) -> bool,
) -> Result<()> {
    let mut copied = HashSet::new();

    for src_dir in src_dirs {
        let src_dir = src_dir.as_ref();
        let files = glob::glob(&format!("{}/{}", src_dir.display(), glob))
            .context("Failed to read source directory")?;
        for file in files {
            let src = file.context("Failed to read source file")?;
            if !src.is_file() {
                // contents will also show up
                continue;
            }
            let suffix = src.strip_prefix(src_dir)?;
            if !predicate(suffix) {
                continue;
            }
            let dst = dst_dir.join(suffix);

            let parent_dir = dst.parent().ok_or(anyhow::anyhow!(
                "Could not get parent directory of destination file"
            ))?;
            fs::create_dir_all(parent_dir).context("Failed to create subdirectory")?;
            fs::copy(&src, &dst).context("Failed to copy file")?;
            let canon = dst
                .canonicalize()
                .context("Failed to canonicalize copied file")?;
            copied.insert(canon);
        }
    }

    // This doesn't do a good job of cleaning up empty directories,
    // but that shouldn't cause any problems right now, since this is
    // only used for header files/libraries/binaries.
    let dst_files = glob::glob(&format!("{}/**/*", dst_dir.display()))
        .context("Failed to read destination directory")?;
    for dst_file in dst_files {
        let dst = dst_file.context("Failed to read destination file")?;
        let canon = dst
            .canonicalize()
            .context("Failed to canonicalize existing file")?;
        if !copied.contains(&canon) && dst.is_file() {
            fs::remove_file(canon).context("Failed to remove stale destination file")?;
        }
    }
    Ok(())
}

/// See the documentation for [`union_glob_with_predicate`] above;
/// this convenience wrapper simply removes the need to explicitly
/// pass a predicate when all files matching the glob should be
/// included.
pub(crate) fn union_glob(
    src_dir: impl Iterator<Item: AsRef<Path>>,
    dst_dir: &Path,
    glob: &str,
) -> Result<()> {
    union_glob_with_predicate(src_dir, dst_dir, glob, |_| true)
}
