use std::{
    fs,
    io::{self, BufWriter, Write},
    path::Path,
};

/// 在同一目录写完并同步才发布。失败或放弃时删除临时文件，不留下看似完整的比赛记录。
pub(super) fn publish(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<&mut fs::File>) -> io::Result<()>,
) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing output directory"))?;
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".arena-")
        .suffix(".part")
        .tempfile_in(dir)?;
    {
        let mut buffer = BufWriter::new(tmp.as_file_mut());
        write(&mut buffer)?;
        buffer.flush()?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist_noclobber(path)?;
    Ok(())
}
