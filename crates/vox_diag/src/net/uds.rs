//! Cross-platform Unix Domain Socket implementation.
//!
//! macOS: re-exports `std::os::unix::net` types.
//! Windows: thin Winsock2 AF_UNIX wrapper over the `windows` 0.62 crate.

#[cfg(unix)]
pub use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};

// ---------------------------------------------------------------------------
// Windows implementation
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod win {
    use std::fmt;
    use std::io::{self, Read, Write};
    use std::mem;
    use std::net::Shutdown;
    use std::os::windows::io::{AsRawSocket, FromRawSocket, OwnedSocket, RawSocket};
    use std::path::Path;
    use std::sync::Once;
    use std::time::Duration;

    use windows::Win32::Networking::WinSock::{
        self as ws, AF_UNIX, FIONBIO, SEND_RECV_FLAGS, SOCKADDR, SOCKET, SOCKET_ERROR,
        SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO, SD_BOTH, SD_RECEIVE, SD_SEND, WSADATA,
        WSAPROTOCOL_INFOW, WSA_FLAG_OVERLAPPED,
    };

    // -- sockaddr_un --------------------------------------------------------
    const UNIX_PATH_MAX: usize = 108;

    #[repr(C)]
    struct SockaddrUn {
        sun_family: u16,
        sun_path: [u8; UNIX_PATH_MAX],
    }

    fn make_sockaddr(path: &Path) -> io::Result<(SockaddrUn, i32)> {
        let path_str = path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not valid UTF-8"))?;
        let path_bytes = path_str.as_bytes();

        if path_bytes.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path"));
        }
        if path_bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains null byte",
            ));
        }
        if path_bytes.len() >= UNIX_PATH_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path exceeds sockaddr_un capacity (108 bytes)",
            ));
        }

        let mut addr = SockaddrUn {
            sun_family: AF_UNIX,
            sun_path: [0u8; UNIX_PATH_MAX],
        };
        addr.sun_path[..path_bytes.len()].copy_from_slice(path_bytes);

        let base = &addr as *const _ as usize;
        let path_field = &addr.sun_path as *const _ as usize;
        let offset = path_field - base;
        let len = (offset + path_bytes.len() + 1) as i32;

        Ok((addr, len))
    }

    // -- WSAStartup ---------------------------------------------------------
    fn init_winsock() {
        static INIT: Once = Once::new();
        INIT.call_once(|| unsafe {
            let mut data: WSADATA = mem::zeroed();
            let ret = ws::WSAStartup(0x0202, &mut data);
            assert!(ret == 0, "WSAStartup failed with code {ret}");
        });
    }

    /// Convert a `windows::core::Error` (HRESULT-wrapped) to `io::Error`
    /// with the correct Win32/WinSock error code.
    fn winsock_err(e: windows::core::Error) -> io::Error {
        let hresult = e.code().0;
        let win32_code = if hresult < 0 {
            // HRESULT_FROM_WIN32 stores the Win32 code in the low 16 bits
            hresult & 0xFFFF
        } else {
            hresult
        };
        io::Error::from_raw_os_error(win32_code)
    }

    fn new_socket() -> io::Result<OwnedSocket> {
        init_winsock();
        let sock = unsafe {
            ws::WSASocketW(AF_UNIX as i32, SOCK_STREAM.0, 0, None, 0, WSA_FLAG_OVERLAPPED)
        }
        .map_err(winsock_err)?;
        Ok(unsafe { OwnedSocket::from_raw_socket(sock.0 as RawSocket) })
    }

    fn socket_handle(sock: &OwnedSocket) -> SOCKET {
        SOCKET(sock.as_raw_socket() as usize)
    }

    // -- SocketAddr ---------------------------------------------------------
    /// Peer address from an accepted UDS connection.
    #[derive(Debug, Clone)]
    pub struct SocketAddr {
        _private: (),
    }

    // -- UnixStream ---------------------------------------------------------
    /// A Unix domain socket stream (Windows Winsock2 AF_UNIX wrapper).
    pub struct UnixStream {
        socket: OwnedSocket,
    }

    impl fmt::Debug for UnixStream {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("UnixStream")
                .field("socket", &self.socket.as_raw_socket())
                .finish()
        }
    }

    impl UnixStream {
        /// Connect to a Unix domain socket at the given path.
        pub fn connect<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let sock = new_socket()?;
            let (addr, len) = make_sockaddr(path.as_ref())?;
            unsafe {
                let ret = ws::connect(
                    socket_handle(&sock),
                    &addr as *const SockaddrUn as *const SOCKADDR,
                    len,
                );
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(Self { socket: sock })
        }

        /// Shut down the read, write, or both halves of the connection.
        pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
            let how = match how {
                Shutdown::Read => SD_RECEIVE,
                Shutdown::Write => SD_SEND,
                Shutdown::Both => SD_BOTH,
            };
            unsafe {
                let ret = ws::shutdown(socket_handle(&self.socket), how);
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        /// Set the socket to blocking or non-blocking mode.
        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            let mut flag: u32 = if nonblocking { 1 } else { 0 };
            unsafe {
                let ret =
                    ws::ioctlsocket(socket_handle(&self.socket), FIONBIO, &mut flag as *mut u32);
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        /// Set the read timeout. `None` removes any existing timeout.
        pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            let millis: u32 = match timeout {
                Some(dur) => {
                    let ms = dur
                        .as_secs()
                        .saturating_mul(1000)
                        .saturating_add(dur.subsec_millis() as u64);
                    if ms == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "zero-duration timeout is not supported",
                        ));
                    }
                    ms.min(u32::MAX as u64) as u32
                }
                None => 0,
            };
            unsafe {
                let ret = ws::setsockopt(
                    socket_handle(&self.socket),
                    SOL_SOCKET,
                    SO_RCVTIMEO,
                    Some(std::slice::from_raw_parts(
                        &millis as *const u32 as *const u8,
                        mem::size_of::<u32>(),
                    )),
                );
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        /// Duplicate the socket handle so it can be used from another thread.
        pub fn try_clone(&self) -> io::Result<Self> {
            let pid = std::process::id();
            unsafe {
                let mut info: WSAPROTOCOL_INFOW = mem::zeroed();
                let ret =
                    ws::WSADuplicateSocketW(socket_handle(&self.socket), pid, &mut info);
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                let sock = ws::WSASocketW(
                    info.iAddressFamily,
                    info.iSocketType,
                    info.iProtocol,
                    Some(&info),
                    0,
                    WSA_FLAG_OVERLAPPED,
                )
                .map_err(winsock_err)?;
                Ok(Self {
                    socket: OwnedSocket::from_raw_socket(sock.0 as RawSocket),
                })
            }
        }
    }

    impl Read for UnixStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            unsafe {
                let ret =
                    ws::recv(socket_handle(&self.socket), buf, SEND_RECV_FLAGS(0));
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                Ok(ret as usize)
            }
        }
    }

    impl Write for UnixStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            unsafe {
                let ret =
                    ws::send(socket_handle(&self.socket), buf, SEND_RECV_FLAGS(0));
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                Ok(ret as usize)
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // Read/Write on &UnixStream (needed for BufReader/BufWriter over &stream)
    impl Read for &UnixStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            unsafe {
                let ret =
                    ws::recv(socket_handle(&self.socket), buf, SEND_RECV_FLAGS(0));
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                Ok(ret as usize)
            }
        }
    }

    impl Write for &UnixStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            unsafe {
                let ret =
                    ws::send(socket_handle(&self.socket), buf, SEND_RECV_FLAGS(0));
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                Ok(ret as usize)
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl AsRawSocket for UnixStream {
        fn as_raw_socket(&self) -> RawSocket {
            self.socket.as_raw_socket()
        }
    }

    impl FromRawSocket for UnixStream {
        unsafe fn from_raw_socket(sock: RawSocket) -> Self {
            Self {
                socket: unsafe { OwnedSocket::from_raw_socket(sock) },
            }
        }
    }

    // -- UnixListener -------------------------------------------------------
    /// A Unix domain socket listener (Windows Winsock2 AF_UNIX wrapper).
    pub struct UnixListener {
        socket: OwnedSocket,
    }

    impl UnixListener {
        /// Bind to the given path and start listening with a backlog of 128.
        pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let sock = new_socket()?;
            let (addr, len) = make_sockaddr(path.as_ref())?;
            unsafe {
                let ret = ws::bind(
                    socket_handle(&sock),
                    &addr as *const SockaddrUn as *const SOCKADDR,
                    len,
                );
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
                let ret = ws::listen(socket_handle(&sock), 128);
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(Self { socket: sock })
        }

        /// Accept a new connection.
        pub fn accept(&self) -> io::Result<(UnixStream, SocketAddr)> {
            let mut storage: SockaddrUn = unsafe { mem::zeroed() };
            let mut len = mem::size_of::<SockaddrUn>() as i32;
            let sock = unsafe {
                ws::accept(
                    socket_handle(&self.socket),
                    Some(&mut storage as *mut SockaddrUn as *mut SOCKADDR),
                    Some(&mut len),
                )
            }
            .map_err(winsock_err)?;
            let stream = UnixStream {
                socket: unsafe { OwnedSocket::from_raw_socket(sock.0 as RawSocket) },
            };
            Ok((stream, SocketAddr { _private: () }))
        }

        /// Set the listener to blocking or non-blocking mode.
        pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
            let mut flag: u32 = if nonblocking { 1 } else { 0 };
            unsafe {
                let ret =
                    ws::ioctlsocket(socket_handle(&self.socket), FIONBIO, &mut flag as *mut u32);
                if ret == SOCKET_ERROR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }

        /// Returns an iterator over incoming connections.
        pub fn incoming(&self) -> Incoming<'_> {
            Incoming { listener: self }
        }
    }

    impl AsRawSocket for UnixListener {
        fn as_raw_socket(&self) -> RawSocket {
            self.socket.as_raw_socket()
        }
    }

    // -- Incoming iterator --------------------------------------------------
    /// An iterator over incoming UDS connections.
    pub struct Incoming<'a> {
        listener: &'a UnixListener,
    }

    impl<'a> Iterator for Incoming<'a> {
        type Item = io::Result<UnixStream>;

        fn next(&mut self) -> Option<Self::Item> {
            Some(self.listener.accept().map(|(stream, _)| stream))
        }
    }
}

#[cfg(windows)]
pub use win::*;

// ---------------------------------------------------------------------------
// Tests (T008)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::thread;

    fn socket_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        dir.join(format!("{name}.sock"))
    }

    #[test]
    fn bind_accept_connect_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(dir.path(), "roundtrip");

        let listener = UnixListener::bind(&path).expect("bind");
        let handle = thread::spawn({
            let path = path.clone();
            move || {
                let _stream = UnixStream::connect(&path).expect("connect");
            }
        });

        let (_accepted, _addr) = listener.accept().expect("accept");
        handle.join().expect("client thread");
    }

    #[test]
    fn send_recv_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(dir.path(), "sendrecv");

        let listener = UnixListener::bind(&path).expect("bind");
        let handle = thread::spawn({
            let path = path.clone();
            move || {
                let mut stream = UnixStream::connect(&path).expect("connect");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .expect("set client read timeout");
                stream.write_all(b"hello from client\n").expect("write");
                let mut buf = String::new();
                BufReader::new(&stream).read_line(&mut buf).expect("read");
                assert_eq!(buf, "hello from server\n");
            }
        });

        let (accepted, _) = listener.accept().expect("accept");
        accepted
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set server read timeout");
        let mut buf = String::new();
        BufReader::new(&accepted).read_line(&mut buf).expect("read");
        assert_eq!(buf, "hello from client\n");
        (&accepted).write_all(b"hello from server\n").expect("write");

        handle.join().expect("client thread");
    }

    #[test]
    fn nonblocking_accept_returns_would_block() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(dir.path(), "nonblock");

        let listener = UnixListener::bind(&path).expect("bind");
        listener.set_nonblocking(true).expect("set_nonblocking");

        match listener.accept() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("expected WouldBlock, got {other:?}"),
        }
    }

    #[test]
    fn shutdown_half_close() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(dir.path(), "shutdown");

        let listener = UnixListener::bind(&path).expect("bind");
        let handle = thread::spawn({
            let path = path.clone();
            move || {
                let mut stream = UnixStream::connect(&path).expect("connect");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .expect("set client read timeout");
                stream.write_all(b"data").expect("write");
                stream
                    .shutdown(std::net::Shutdown::Write)
                    .expect("shutdown write");
                let mut buf = [0u8; 4];
                let n = stream.read(&mut buf).expect("read after shutdown");
                assert_eq!(&buf[..n], b"back");
            }
        });

        let (mut accepted, _) = listener.accept().expect("accept");
        accepted
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("set server read timeout");
        let mut buf = [0u8; 4];
        let n = accepted.read(&mut buf).expect("read");
        assert_eq!(&buf[..n], b"data");
        let n2 = accepted.read(&mut buf).expect("read after client shutdown");
        assert_eq!(n2, 0);
        (&accepted).write_all(b"back").expect("write back");

        handle.join().expect("client thread");
    }

    #[test]
    fn read_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = socket_path(dir.path(), "timeout");

        let listener = UnixListener::bind(&path).expect("bind");
        let _handle = thread::spawn({
            let path = path.clone();
            move || {
                let _stream = UnixStream::connect(&path).expect("connect");
                thread::sleep(std::time::Duration::from_secs(5));
            }
        });

        let (mut accepted, _) = listener.accept().expect("accept");
        accepted
            .set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .expect("set timeout");

        let mut buf = [0u8; 16];
        let result = accepted.read(&mut buf);
        assert!(result.is_err(), "expected timeout error, got {result:?}");
        let err = result.unwrap_err();
        assert!(
            err.kind() == std::io::ErrorKind::WouldBlock
                || err.kind() == std::io::ErrorKind::TimedOut,
            "expected WouldBlock or TimedOut, got {:?}",
            err.kind()
        );
    }
}
