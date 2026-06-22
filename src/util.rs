use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub(crate) fn copy_glob_with_predicate(
    src_dir: &Path,
    dst_dir: &Path,
    glob: &str,
    predicate: impl Fn(&Path) -> bool,
) -> Result<()> {
    let files = glob::glob(&format!("{}/{}", src_dir.display(), glob))
        .context("Failed to read include source directory")?;
    for file in files {
        let src = file.context("Failed to read include source file")?;
        let dst = src.strip_prefix(src_dir).unwrap();
        if !predicate(dst) {
            continue;
        }
        let dst = dst_dir.join(dst);

        fs::create_dir_all(dst.parent().unwrap())
            .context("Failed to create include subdirectory")?;
        fs::copy(&src, &dst).context("Failed to copy include file")?;
    }
    Ok(())
}

pub(crate) fn copy_glob(src_dir: &Path, dst_dir: &Path, glob: &str) -> Result<()> {
    copy_glob_with_predicate(src_dir, dst_dir, glob, |_| true)
}
