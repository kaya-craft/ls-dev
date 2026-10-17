//! Log streams: Docker containers and files of managed processes.
//! One session at a time; dropping the session stops its task.

use std::path::PathBuf;
use std::time::Duration;

use bollard::container::{LogOutput, LogsOptions};
use bollard::Docker;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const TAIL_LINES: usize = 500;
const BATCH_EVERY: Duration = Duration::from_millis(100);
const FILE_POLL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub stream: &'static str,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogChunk {
    pub id: String,
    pub lines: Vec<LogLine>,
}

/// Turns arbitrary chunks into complete lines; keeps the trailing partial line.
#[derive(Debug, Default)]
pub struct LineSplitter {
    partial: String,
}

impl LineSplitter {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.partial.push_str(chunk);
        let mut lines = Vec::new();
        while let Some(i) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=i).collect();
            lines.push(line.trim_end_matches(['\n', '\r']).to_string());
        }
        lines
    }
}

pub fn last_lines(lines: Vec<String>, n: usize) -> Vec<String> {
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).collect()
}

/// Forward a Docker log stream as batches of lines, every 100 ms.
/// Returns why the stream stopped.
pub async fn pump<S>(stream: S, mut emit: impl FnMut(Vec<LogLine>)) -> String
where
    S: Stream<Item = Result<LogOutput, bollard::errors::Error>> + Unpin,
{
    let mut stream = stream;
    let mut out = LineSplitter::default();
    let mut err = LineSplitter::default();
    let mut batch: Vec<LogLine> = Vec::new();
    let mut tick = tokio::time::interval(BATCH_EVERY);
    let reason = loop {
        tokio::select! {
            item = stream.next() => match item {
                Some(Ok(LogOutput::StdOut { message } | LogOutput::Console { message })) => {
                    let text = String::from_utf8_lossy(&message);
                    batch.extend(out.push(&text).into_iter().map(|line| LogLine { stream: "stdout", line }));
                }
                Some(Ok(LogOutput::StdErr { message })) => {
                    let text = String::from_utf8_lossy(&message);
                    batch.extend(err.push(&text).into_iter().map(|line| LogLine { stream: "stderr", line }));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => break e.to_string(),
                None => break "stream ended".to_string(),
            },
            _ = tick.tick() => {
                if !batch.is_empty() {
                    emit(std::mem::take(&mut batch));
                }
            }
        }
    };
    if !batch.is_empty() {
        emit(batch);
    }
    reason
}

/// Send the last `initial` lines of the file, then every appended line.
/// Starts over when the file shrinks.
pub async fn tail_file(
    path: PathBuf,
    initial: usize,
    poll: Duration,
    mut emit: impl FnMut(Vec<LogLine>),
) {
    let to_lines = |v: Vec<String>| {
        v.into_iter()
            .map(|line| LogLine {
                stream: "stdout",
                line,
            })
            .collect::<Vec<_>>()
    };
    let mut splitter = LineSplitter::default();
    let text = tokio::fs::read(&path).await.unwrap_or_default();
    let mut offset = text.len() as u64;
    let first = last_lines(splitter.push(&String::from_utf8_lossy(&text)), initial);
    if !first.is_empty() {
        emit(to_lines(first));
    }
    loop {
        tokio::time::sleep(poll).await;
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            continue;
        };
        if meta.len() < offset {
            offset = 0;
            splitter = LineSplitter::default();
        }
        if meta.len() == offset {
            continue;
        }
        let Ok(mut file) = tokio::fs::File::open(&path).await else {
            continue;
        };
        if file.seek(std::io::SeekFrom::Start(offset)).await.is_err() {
            continue;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).await.is_err() {
            continue;
        }
        offset += buf.len() as u64;
        let lines = splitter.push(&String::from_utf8_lossy(&buf));
        if !lines.is_empty() {
            emit(to_lines(lines));
        }
    }
}

pub struct LogSession {
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for LogSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn emitter(app: AppHandle, id: String) -> impl FnMut(Vec<LogLine>) {
    move |lines| {
        let _ = app.emit(
            "logs:chunk",
            LogChunk {
                id: id.clone(),
                lines,
            },
        );
    }
}

pub fn open_docker(app: AppHandle, docker: Docker, id: String, container_id: String) -> LogSession {
    let task = tauri::async_runtime::spawn(async move {
        let stream = docker.logs(
            &container_id,
            Some(LogsOptions::<String> {
                follow: true,
                stdout: true,
                stderr: true,
                timestamps: true,
                tail: TAIL_LINES.to_string(),
                ..Default::default()
            }),
        );
        let reason = pump(stream, emitter(app.clone(), id.clone())).await;
        let _ = app.emit(
            "logs:closed",
            serde_json::json!({ "id": id, "reason": reason }),
        );
    });
    LogSession { task }
}

pub fn open_file(app: AppHandle, id: String, path: PathBuf) -> LogSession {
    let task = tauri::async_runtime::spawn(async move {
        tail_file(path, TAIL_LINES, FILE_POLL, emitter(app, id)).await;
    });
    LogSession { task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::sync::{Arc, Mutex};

    #[test]
    fn splitter_keeps_partial_lines_until_the_newline() {
        let mut s = LineSplitter::default();
        assert_eq!(s.push("a\nb"), vec!["a"]);
        assert_eq!(s.push("c\r\nd\n"), vec!["bc", "d"]);
        assert!(s.push("").is_empty());
    }

    #[test]
    fn last_lines_keeps_the_tail() {
        let lines: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(last_lines(lines, 3), vec!["7", "8", "9"]);
    }

    #[tokio::test]
    async fn pump_splits_each_stream_and_reports_the_end() {
        let items: Vec<Result<LogOutput, bollard::errors::Error>> = vec![
            Ok(LogOutput::StdOut {
                message: Bytes::from("one\ntw"),
            }),
            Ok(LogOutput::StdErr {
                message: Bytes::from("oops\n"),
            }),
            Ok(LogOutput::StdOut {
                message: Bytes::from("o\n"),
            }),
        ];
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let reason = pump(futures_util::stream::iter(items), move |lines| {
            sink.lock().unwrap().extend(lines)
        })
        .await;
        let got: Vec<(String, String)> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|l| (l.stream.to_string(), l.line.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("stdout".into(), "one".into()),
                ("stderr".into(), "oops".into()),
                ("stdout".into(), "two".into()),
            ]
        );
        assert_eq!(reason, "stream ended");
    }

    #[tokio::test]
    async fn tail_file_sends_the_tail_then_appended_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.log");
        std::fs::write(&path, "1\n2\n3\n").unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<String>>();
        let task = tokio::spawn(tail_file(
            path.clone(),
            2,
            Duration::from_millis(20),
            move |lines| {
                let _ = tx.send(lines.into_iter().map(|l| l.line).collect());
            },
        ));
        assert_eq!(rx.recv().await.unwrap(), vec!["2", "3"]);
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "4").unwrap();
        assert_eq!(rx.recv().await.unwrap(), vec!["4"]);
        task.abort();
    }
}
