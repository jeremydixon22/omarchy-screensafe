//! Optional prototype-only decoder channel. No codec code or new process
//! authority in the renderer. Bounded RGBA delivery becomes owned memory before
//! entering the ordinary compositor and its runtime flash guard.
use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileExt, FileTypeExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

const MAGIC: u32 = 0x32565353;
static CLIENT: OnceLock<Mutex<Client>> = OnceLock::new();
static PREVIEW_INPUT: OnceLock<File> = OnceLock::new();

fn inherited_preview_input(number: u32) -> Result<File, String> {
    if !(3..=1023).contains(&number) {
        return Err("invalid preview input descriptor".into());
    }
    // Reopening a pipe through procfs is safe Rust and keeps the decoder's
    // private stdin socket separate from the panel's bounded text controls.
    let path = format!("/proc/self/fd/{number}");
    let kind = std::fs::metadata(&path).map_err(|e| e.to_string())?.file_type();
    if !kind.is_fifo() && !kind.is_file() {
        return Err("preview input must be a pipe or regular file".into());
    }
    let input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| format!("can't retain preview input: {e}"))?;
    let kind = input.metadata().map_err(|e| e.to_string())?.file_type();
    if !kind.is_fifo() && !kind.is_file() {
        return Err("preview input must be a pipe or regular file".into());
    }
    Ok(input)
}

pub fn preview_input() -> Result<File, String> {
    match PREVIEW_INPUT.get() {
        Some(input) => input.try_clone().map_err(|e| e.to_string()),
        None => std::io::stdin().as_fd().try_clone_to_owned().map(File::from).map_err(|e| e.to_string()),
    }
}

pub fn available() -> bool {
    CLIENT.get().is_some()
}

struct Client {
    channel: UnixStream,
    sequence: u32,
    failed: bool,
    pipe: bool,
    cache: VecDeque<(u64, u16, Vec<u8>)>,
    pixel_deadline: Duration,
}

/// Called before filesystem/syscall confinement, only by the explicit local
/// prototype wrapper. Ordinary plugin invocations never acquire this channel.
pub fn initialize() -> Result<(), String> {
    if std::env::var("SCREENSAFE_SSV2_WORKER").as_deref() != Ok("1") {
        return Ok(());
    }
    if let Ok(raw) = std::env::var("SCREENSAFE_PREVIEW_INPUT_FD") {
        let number = raw.parse::<u32>().map_err(|_| "invalid preview input descriptor")?;
        PREVIEW_INPUT.set(inherited_preview_input(number)?).map_err(|_| "duplicate preview input")?;
    }
    let fd = std::io::stdin().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    if rustix::net::sockopt::socket_type(&fd).map_err(|e| e.to_string())? != rustix::net::SocketType::SEQPACKET {
        return Err("SSVv2 needs its private sequenced-packet channel".into());
    }
    let channel = UnixStream::from(fd);
    channel.set_read_timeout(Some(Duration::from_secs(8))).map_err(|e| e.to_string())?;
    channel.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    let pipe = std::env::var("SCREENSAFE_SSV2_PIPE").as_deref() == Ok("1");
    CLIENT
        .set(Mutex::new(Client { channel, sequence: 0, failed: false, pipe, cache: VecDeque::new(), pixel_deadline: Duration::from_secs(5) }))
        .map_err(|_| "duplicate SSVv2 channel".to_string())
}

pub fn frame(file: &File, frame: u16, width: u32, height: u32, identity: u64) -> Result<Vec<u8>, String> {
    let mut client = CLIENT.get().ok_or("SSVv2 requires the explicit prototype decoder wrapper")?.lock().map_err(|_| "SSVv2 channel poisoned")?;
    if client.failed {
        return Err("SSVv2 channel is closed after a protocol failure".into());
    }
    let result = client.frame(file, frame, width, height, identity);
    if client.failed {
        let _ = client.channel.shutdown(std::net::Shutdown::Both);
    }
    result
}

impl Client {
    fn frame(&mut self, file: &File, frame: u16, width: u32, height: u32, identity: u64) -> Result<Vec<u8>, String> {
        use rustix::net::{
            recvmsg, sendmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
        };
        if let Some(at) = self.cache.iter().position(|(id, index, _)| *id == identity && *index == frame) {
            let entry = self.cache.remove(at).unwrap();
            let owned = entry.2.clone();
            self.cache.push_back(entry);
            return Ok(owned);
        }
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
        if words[..3] != [MAGIC, self.sequence, frame as u32] || words[7] > u32::from(self.pipe) || words[6] > 1 {
            return Err("SSVv2 decoder returned an invalid identity".into());
        }
        if words[6] == 1 {
            if words[3..6] != [0, 0, 0] || words[7] != 0 || !descriptors.is_empty() {
                return Err("invalid SSVv2 refusal".into());
            }
            self.failed = false;
            return Err("SSVv2 decoder refused this frame".into());
        }
        if words[3..6] != [width, height, size] || descriptors.len() != 1 || size > 1280 * 720 * 4 {
            return Err("SSVv2 pixel dimensions/descriptor count changed".into());
        }
        let fd = descriptors.pop().unwrap();
        let mut owned = vec![0; size as usize];
        if words[7] == 0 {
            let file = File::from(fd);
            let meta = file.metadata().map_err(|e| e.to_string())?;
            let required = rustix::fs::SealFlags::WRITE | rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK | rustix::fs::SealFlags::SEAL;
            if !meta.is_file() || meta.len() != size as u64 || !rustix::fs::fcntl_get_seals(&file).map_err(|e| e.to_string())?.contains(required) {
                return Err("SSVv2 pixels are not an immutable exact-sized snapshot".into());
            }
            file.read_exact_at(&mut owned, 0).map_err(|e| e.to_string())?;
        } else {
            if rustix::net::sockopt::socket_type(&fd).map_err(|e| e.to_string())? != rustix::net::SocketType::STREAM
                || rustix::net::getsockname(&fd).map_err(|e| e.to_string())?.address_family() != rustix::net::AddressFamily::UNIX
            {
                return Err("SSVv2 pixel stream is not a private Unix stream".into());
            }
            let mut stream = UnixStream::from(fd);
            let deadline = Instant::now() + self.pixel_deadline;
            let mut at = 0;
            while at < owned.len() {
                let left = deadline.checked_duration_since(Instant::now()).ok_or("SSVv2 pixel deadline exceeded")?;
                stream.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
                match stream.read(&mut owned[at..]) {
                    Ok(0) => return Err("SSVv2 pixel stream ended early".into()),
                    Ok(n) => at += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(format!("SSVv2 pixel stream failed: {e}")),
                }
            }
        }
        if self.pipe {
            self.cache.push_back((identity, frame, owned.clone()));
            while self.cache.len() > 12 {
                self.cache.pop_front();
            }
        }
        self.failed = false;
        Ok(owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn preview_controls_use_a_retained_pipe_and_never_the_decoder_socket() {
        use std::os::fd::AsRawFd;
        let (reader, mut writer) = std::io::pipe().unwrap();
        let mut retained = inherited_preview_input(reader.as_raw_fd() as u32).unwrap();
        drop(reader);
        writer.write_all(b"settings style=2\nquit\n").unwrap();
        drop(writer);
        let mut text = String::new();
        retained.read_to_string(&mut text).unwrap();
        assert_eq!(text, "settings style=2\nquit\n");
        let (socket, _) = UnixStream::pair().unwrap();
        assert!(inherited_preview_input(socket.as_raw_fd() as u32).is_err());
        assert!(inherited_preview_input(0).is_err());
    }

    fn pair() -> (Client, UnixStream) {
        let (a, b) =
            rustix::net::socketpair(rustix::net::AddressFamily::UNIX, rustix::net::SocketType::SEQPACKET, rustix::net::SocketFlags::CLOEXEC, None).unwrap();
        let channel = UnixStream::from(a);
        channel.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        (Client { channel, sequence: 0, failed: false, pipe: false, cache: VecDeque::new(), pixel_deadline: Duration::from_millis(50) }, UnixStream::from(b))
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
            let result = client.frame(&input, 7, 2, 2, 1);
            assert_eq!(result.is_ok(), valid);
            assert_eq!(client.failed, !valid && !refusal);
            if let Ok(pixels) = result {
                assert_eq!(pixels, vec![127; 16]);
            }
            worker.join().unwrap();
        }
    }

    #[test]
    fn direct_stream_becomes_owned_before_guarding_and_has_a_total_deadline() {
        use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
        for partial in [false, true] {
            let (mut client, peer) = pair();
            client.pipe = true;
            let worker = std::thread::spawn(move || {
                let mut request = [0; 16];
                (&peer).read_exact(&mut request).unwrap();
                let sequence = u32::from_le_bytes(request[4..8].try_into().unwrap());
                let (reader, mut writer) = UnixStream::pair().unwrap();
                let bytes: Vec<u8> = [MAGIC, sequence, 7, 2, 2, 16, 0, 1].iter().flat_map(|v| v.to_le_bytes()).collect();
                let rights = [reader.as_fd()];
                let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
                let mut control = SendAncillaryBuffer::new(&mut space);
                assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
                sendmsg(&peer, &[std::io::IoSlice::new(&bytes)], &mut control, SendFlags::NOSIGNAL).unwrap();
                if partial {
                    writer.write_all(&[127; 8]).unwrap();
                    std::thread::sleep(Duration::from_millis(100));
                } else {
                    writer.write_all(&[127; 16]).unwrap();
                    writer.write_all(&[0; 16]).unwrap();
                }
            });
            let source = File::open("/dev/null").unwrap();
            let result = client.frame(&source, 7, 2, 2, 99);
            assert_eq!(result.is_err(), partial);
            if let Ok(mut owned) = result {
                assert_eq!(owned, vec![127; 16]);
                owned[0] = 0;
                assert_eq!(client.frame(&source, 7, 2, 2, 99).unwrap(), vec![127; 16]);
            }
            worker.join().unwrap();
        }
    }

    #[test]
    fn hostile_replies_never_admit_pixels_or_leave_the_channel_usable() {
        use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
        for case in [
            "magic",
            "sequence",
            "dimensions",
            "status",
            "transport",
            "missing-fd",
            "extra-fd",
            "short",
            "oversized",
            "wrong-size",
            "partial-seals",
            "device-fd",
            "datagram-fd",
            "foreign-stream",
            "refusal-with-fd",
        ] {
            let (mut client, peer) = pair();
            client.pipe = true;
            let worker = std::thread::spawn(move || {
                let mut request = [0; 16];
                (&peer).read_exact(&mut request).unwrap();
                let sequence = u32::from_le_bytes(request[4..8].try_into().unwrap());
                let mut words = [MAGIC, sequence, 7, 2, 2, 16, 0, 0];
                match case {
                    "magic" => words[0] = 0,
                    "sequence" => words[1] += 1,
                    "dimensions" => words[3] = 1280,
                    "status" => words[6] = 2,
                    "transport" => words[7] = 2,
                    "datagram-fd" | "foreign-stream" => words[7] = 1,
                    "refusal-with-fd" => words[6] = 1,
                    _ => (),
                }
                let mut bytes: Vec<u8> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
                if case == "short" {
                    bytes.pop();
                }
                if case == "oversized" {
                    bytes.push(0);
                }
                let fd = rustix::fs::memfd_create("hostile-video", rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING).unwrap();
                let mut file = File::from(fd);
                file.write_all(&[127; 16]).unwrap();
                if case == "wrong-size" {
                    file.set_len(32).unwrap();
                }
                let seals = if case == "partial-seals" {
                    rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK | rustix::fs::SealFlags::SEAL
                } else {
                    rustix::fs::SealFlags::WRITE | rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK | rustix::fs::SealFlags::SEAL
                };
                rustix::fs::fcntl_add_seals(&file, seals).unwrap();
                if case == "device-fd" {
                    file = File::open("/dev/null").unwrap();
                }
                let (datagram, _) = std::os::unix::net::UnixDatagram::pair().unwrap();
                let foreign = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let selected = match case {
                    "datagram-fd" => datagram.as_fd(),
                    "foreign-stream" => foreign.as_fd(),
                    _ => file.as_fd(),
                };
                let rights = match case {
                    "missing-fd" => vec![],
                    "extra-fd" => vec![selected, selected],
                    _ => vec![selected],
                };
                let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
                let mut control = SendAncillaryBuffer::new(&mut space);
                if !rights.is_empty() {
                    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
                }
                sendmsg(&peer, &[std::io::IoSlice::new(&bytes)], &mut control, SendFlags::NOSIGNAL).unwrap();
            });
            let source = File::open("/dev/null").unwrap();
            assert!(client.frame(&source, 7, 2, 2, 100).is_err(), "{case}");
            assert!(client.failed, "{case}");
            assert!(client.cache.is_empty(), "{case}");
            worker.join().unwrap();
        }
    }
}
