use std::io::{self, Read, Write};

const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024; // 64 MB

pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame size {len} exceeds maximum {MAX_FRAME_SIZE}"),
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

pub fn write_frame<W: Write>(writer: &mut W, data: &[u8]) -> io::Result<()> {
    if data.len() > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("frame size {} exceeds maximum {MAX_FRAME_SIZE}", data.len()),
        ));
    }
    let len_buf = (data.len() as u32).to_le_bytes();
    writer.write_all(&len_buf)?;
    writer.write_all(data)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_framing_roundtrip() {
        let mut buffer = Vec::new();
        let payload = b"Hello GPUI Daemon!";
        write_frame(&mut buffer, payload).unwrap();

        let mut cursor = Cursor::new(buffer);
        let read = read_frame(&mut cursor).unwrap();
        assert_eq!(read, payload);
    }

    #[test]
    fn test_multiple_frames() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, b"Frame 1").unwrap();
        write_frame(&mut buffer, b"Frame 2").unwrap();

        let mut cursor = Cursor::new(buffer);
        assert_eq!(read_frame(&mut cursor).unwrap(), b"Frame 1");
        assert_eq!(read_frame(&mut cursor).unwrap(), b"Frame 2");
    }
}
