//! Optional prototype-only CPU decoder channel. No codec code or new process
//! authority in the renderer. Only fully write-sealed, bounded RGBA snapshots
//! enter the ordinary compositor and its runtime flash guard.
use std::fs::File;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const MAGIC: u32 = 0x32565353;
static CLIENT: OnceLock<Mutex<Client>> = OnceLock::new();

pub fn available() -> bool {
    CLIENT.get().is_some()
}

struct Client {
    channel: UnixStream,
    sequence: u32,
    failed: bool,
}

/// Called before filesystem/syscall confinement, only by the explicit local
/// prototype wrapper. Ordinary plugin invocations never acquire this channel.
pub fn initialize() -> Result<(), String> {
    if std::env::var("SCREENSAFE_SSV2_WORKER").as_deref() != Ok("1") {
        return Ok(());
    }
    let fd = std::io::stdin().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    if rustix::net::sockopt::socket_type(&fd).map_err(|e| e.to_string())? != rustix::net::SocketType::SEQPACKET {
        return Err("SSVv2 needs its private sequenced-packet channel".into());
    }
    let channel = UnixStream::from(fd);
    channel.set_read_timeout(Some(Duration::from_secs(8))).map_err(|e| e.to_string())?;
    channel.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    CLIENT.set(Mutex::new(Client { channel, sequence: 0, failed: false })).map_err(|_| "duplicate SSVv2 channel".to_string())
}

pub fn frame(file: &File, frame: u16, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut client = CLIENT.get().ok_or("SSVv2 requires the explicit prototype decoder wrapper")?.lock().map_err(|_| "SSVv2 channel poisoned")?;
    if client.failed {
        return Err("SSVv2 channel is closed after a protocol failure".into());
    }
    let result = client.frame(file, frame, width, height);
    if client.failed {
        let _ = client.channel.shutdown(std::net::Shutdown::Both);
    }
    result
}

impl Client {
    fn frame(&mut self, file: &File, frame: u16, width: u32, height: u32) -> Result<Vec<u8>, String> {
        use rustix::net::{
            recvmsg, sendmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
        };
        self.failed = true;
        self.sequence = self.sequence.wrapping_add(1);
        let request: Vec<u8> = [MAGIC, self.sequence, frame as u32, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rights = [file.as_fd()];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !control.push(SendAncillaryMessage::ScmRights(&rights)) {
            return Err("can't attach video input descriptor".into());
        }
        let sent = sendmsg(&self.channel, &[std::io::IoSlice::new(&request)], &mut control, SendFlags::NOSIGNAL).map_err(|e| e.to_string())?;
        if sent != request.len() {
            return Err("short SSVv2 request".into());
        }
        let mut response = [0u8; 32];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let received =
            recvmsg(&self.channel, &mut [std::io::IoSliceMut::new(&mut response)], &mut control, RecvFlags::CMSG_CLOEXEC).map_err(|e| e.to_string())?;
        let mut descriptors = Vec::new();
        for item in control.drain() {
            match item {
                RecvAncillaryMessage::ScmRights(rights) => descriptors.extend(rights),
                _ => return Err("unexpected SSVv2 channel metadata".into()),
            }
        }
        if received.bytes != response.len() || received.flags.intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC) {
            return Err("invalid SSVv2 response length".into());
        }
        let words: Vec<u32> = response.chunks_exact(4).map(|v| u32::from_le_bytes(v.try_into().unwrap())).collect();
        let size = width.checked_mul(height).and_then(|n| n.checked_mul(4)).ok_or("video pixel size overflow")?;
        if words[..3] != [MAGIC, self.sequence, frame as u32] || words[7] != 0 || words[6] > 1 {
            return Err("SSVv2 decoder returned an invalid identity".into());
        }
        if words[6] == 1 {
            if words[3..6] != [0, 0, 0] || !descriptors.is_empty() {
                return Err("invalid SSVv2 refusal".into());
            }
            self.failed = false;
            return Err("SSVv2 decoder refused this frame".into());
        }
        if words[3..6] != [width, height, size] || descriptors.len() != 1 || size > 1280 * 720 * 4 {
            return Err("SSVv2 pixel dimensions/descriptor count changed".into());
        }
        let file = File::from(descriptors.pop().unwrap());
        let meta = file.metadata().map_err(|e| e.to_string())?;
        let required = rustix::fs::SealFlags::WRITE | rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK | rustix::fs::SealFlags::SEAL;
        if !meta.is_file() || meta.len() != size as u64 || !rustix::fs::fcntl_get_seals(&file).map_err(|e| e.to_string())?.contains(required) {
            return Err("SSVv2 pixels are not an immutable exact-sized snapshot".into());
        }
        let mut owned = vec![0; size as usize];
        file.read_exact_at(&mut owned, 0).map_err(|e| e.to_string())?;
        self.failed = false;
        Ok(owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn pair() -> (Client, UnixStream) {
        let (a, b) =
            rustix::net::socketpair(rustix::net::AddressFamily::UNIX, rustix::net::SocketType::SEQPACKET, rustix::net::SocketFlags::CLOEXEC, None).unwrap();
        let channel = UnixStream::from(a);
        channel.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        (Client { channel, sequence: 0, failed: false }, UnixStream::from(b))
    }

    fn respond(peer: UnixStream, sealed: bool, frame: u32, refusal: bool) {
        use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
        let mut request = [0; 16];
        (&peer).read_exact(&mut request).unwrap();
        let sequence = u32::from_le_bytes(request[4..8].try_into().unwrap());
        if refusal {
            let bytes: Vec<u8> = [MAGIC, sequence, frame, 0, 0, 0, 1, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
            (&peer).write_all(&bytes).unwrap();
            return;
        }
        let fd = rustix::fs::memfd_create("video-test", rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING).unwrap();
        let mut file = File::from(fd);
        file.write_all(&[127; 16]).unwrap();
        if sealed {
            rustix::fs::fcntl_add_seals(
                &file,
                rustix::fs::SealFlags::WRITE | rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK | rustix::fs::SealFlags::SEAL,
            )
            .unwrap();
            assert!(file.write_all(&[0]).is_err());
        }
        let bytes: Vec<u8> = [MAGIC, sequence, frame, 2, 2, 16, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let rights = [file.as_fd()];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut ancillary = SendAncillaryBuffer::new(&mut space);
        assert!(ancillary.push(SendAncillaryMessage::ScmRights(&rights)));
        sendmsg(&peer, &[std::io::IoSlice::new(&bytes)], &mut ancillary, SendFlags::NOSIGNAL).unwrap();
    }

    #[test]
    fn compressed_pixels_must_be_fully_sealed_and_identify_the_requested_frame() {
        for (sealed, frame, refusal, valid) in [(true, 7, false, true), (false, 7, false, false), (true, 8, false, false), (false, 7, true, false)] {
            let (mut client, peer) = pair();
            let worker = std::thread::spawn(move || respond(peer, sealed, frame, refusal));
            let input = File::open("/dev/null").unwrap();
            let result = client.frame(&input, 7, 2, 2);
            assert_eq!(result.is_ok(), valid);
            assert_eq!(client.failed, !valid && !refusal);
            if let Ok(pixels) = result {
                assert_eq!(pixels, vec![127; 16]);
            }
            worker.join().unwrap();
        }
    }
}
