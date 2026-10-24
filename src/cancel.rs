//! 可取消的阻塞 HTTP 适配。网络线程只持有 socket 与有限缓冲区，不写文件；
//! 取消后任务线程立即返回，旧请求即使稍后完成也不能修改环境或发布状态。
use anyhow::{anyhow, Result};
use std::{
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

pub(crate) fn request(
    request: reqwest::blocking::RequestBuilder,
    cancel: &AtomicBool,
) -> Result<reqwest::blocking::Response> {
    crate::launcher::ensure_not_cancelled(cancel)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(request.send());
    });
    loop {
        crate::launcher::ensure_not_cancelled(cancel)?;
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return Ok(result?),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => return Err(anyhow!(error)),
        }
    }
}

pub(crate) struct Body {
    chunks: mpsc::Receiver<io::Result<Vec<u8>>>,
    current: io::Cursor<Vec<u8>>,
    cancel: Arc<AtomicBool>,
    eof: bool,
}

pub(crate) fn body(mut response: reqwest::blocking::Response, cancel: Arc<AtomicBool>) -> Body {
    let (sender, chunks) = mpsc::sync_channel(2);
    let stop = cancel.clone();
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let mut buffer = vec![0; 128 * 1024];
            let result = response.read(&mut buffer).map(|size| {
                buffer.truncate(size);
                buffer
            });
            let done = result.as_ref().map_or(true, |buffer| buffer.is_empty());
            if sender.send(result).is_err() || done {
                break;
            }
        }
    });
    Body {
        chunks,
        current: io::Cursor::new(Vec::new()),
        cancel,
        eof: false,
    }
}

impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "cancelled",
                ));
            }
            let size = self.current.read(buffer)?;
            if size != 0 || self.eof {
                return Ok(size);
            }
            match self.chunks.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => {
                    let chunk = result?;
                    self.eof = chunk.is_empty();
                    self.current = io::Cursor::new(chunk);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "HTTP reader ended",
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, time::Instant};

    #[test]
    fn cancellation_interrupts_a_silent_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n")
                .unwrap();
            thread::sleep(Duration::from_millis(500));
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let response = request(client.get(url), &cancel).unwrap();
        let body = body(response, cancel.clone());
        let signal = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            signal.store(true, Ordering::SeqCst);
        });
        let now = Instant::now();
        // BufRead must propagate cancellation rather than endlessly retry EINTR.
        let mut reader = std::io::BufReader::new(body);
        let error = std::io::BufRead::read_line(&mut reader, &mut String::new()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(now.elapsed() < Duration::from_millis(400));
        server.join().unwrap();
    }
}
