//! Assembles the messages of a simple-protocol query into a bounded result.
//! Independent of tokio-postgres types.

use serde::Serialize;

pub const MAX_ROWS: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Columns(Vec<String>),
    Row(Vec<Option<String>>),
    Done(u64),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementSummary {
    pub rows: u64,
    pub returned_columns: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    pub statements: Vec<StatementSummary>,
    pub result: Option<ResultSet>,
    pub elapsed_ms: u64,
    /// Set by the session when stopping at the row limit undid earlier work.
    pub rolled_back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    Stop,
}

pub struct Assembler {
    statements: Vec<StatementSummary>,
    current: Option<ResultSet>,
    columns_since_done: bool,
    stopped: bool,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            statements: Vec::new(),
            current: None,
            columns_since_done: false,
            stopped: false,
        }
    }

    pub fn push(&mut self, m: Message) -> Step {
        if self.stopped {
            return Step::Stop;
        }
        match m {
            Message::Columns(columns) => {
                self.current = Some(ResultSet {
                    columns,
                    rows: Vec::new(),
                    truncated: false,
                });
                self.columns_since_done = true;
            }
            Message::Row(values) => {
                if let Some(current) = self.current.as_mut() {
                    if current.rows.len() >= MAX_ROWS {
                        current.truncated = true;
                        self.stopped = true;
                        self.statements.push(StatementSummary {
                            rows: MAX_ROWS as u64,
                            returned_columns: true,
                        });
                        return Step::Stop;
                    }
                    current.rows.push(values);
                }
            }
            Message::Done(rows) => {
                self.statements.push(StatementSummary {
                    rows,
                    returned_columns: self.columns_since_done,
                });
                self.columns_since_done = false;
            }
        }
        Step::Continue
    }

    pub fn finish(self, elapsed_ms: u64) -> RunOutput {
        RunOutput {
            statements: self.statements,
            result: self.current,
            elapsed_ms,
            rolled_back: false,
        }
    }
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(v: &[Option<&str>]) -> Message {
        Message::Row(v.iter().map(|x| x.map(String::from)).collect())
    }

    fn cols(v: &[&str]) -> Message {
        Message::Columns(v.iter().map(|x| x.to_string()).collect())
    }

    #[test]
    fn finish_is_not_rolled_back_and_serializes_camel_case() {
        let mut a = Assembler::new();
        a.push(Message::Done(0));
        let out = a.finish(1);
        assert!(!out.rolled_back);
        assert_eq!(
            serde_json::to_value(&out).unwrap()["rolledBack"],
            serde_json::json!(false)
        );
    }

    #[test]
    fn select_produces_result_and_summary() {
        let mut a = Assembler::new();
        a.push(cols(&["a", "b"]));
        a.push(row(&[Some("1"), Some("2")]));
        a.push(row(&[Some("3"), Some("4")]));
        a.push(Message::Done(2));
        let out = a.finish(7);
        assert_eq!(
            out.statements,
            vec![StatementSummary {
                rows: 2,
                returned_columns: true
            }]
        );
        let r = out.result.unwrap();
        assert_eq!(r.columns, vec!["a", "b"]);
        assert_eq!(r.rows.len(), 2);
        assert!(!r.truncated);
        assert_eq!(out.elapsed_ms, 7);
    }

    #[test]
    fn update_alone_has_no_result() {
        let mut a = Assembler::new();
        a.push(Message::Done(3));
        let out = a.finish(0);
        assert_eq!(
            out.statements,
            vec![StatementSummary {
                rows: 3,
                returned_columns: false
            }]
        );
        assert!(out.result.is_none());
    }

    #[test]
    fn last_result_with_columns_wins() {
        let mut a = Assembler::new();
        a.push(cols(&["x"]));
        a.push(row(&[Some("first")]));
        a.push(Message::Done(1));
        a.push(cols(&["y"]));
        a.push(row(&[Some("second")]));
        a.push(Message::Done(1));
        a.push(Message::Done(4));
        let out = a.finish(0);
        assert_eq!(out.statements.len(), 3);
        assert!(!out.statements[2].returned_columns);
        let r = out.result.unwrap();
        assert_eq!(r.columns, vec!["y"]);
        assert_eq!(r.rows, vec![vec![Some("second".to_string())]]);
    }

    #[test]
    fn null_and_empty_string_are_distinct() {
        let mut a = Assembler::new();
        a.push(cols(&["a", "b"]));
        a.push(row(&[None, Some("")]));
        a.push(Message::Done(1));
        let r = a.finish(0).result.unwrap();
        assert_eq!(r.rows[0], vec![None, Some(String::new())]);
    }

    #[test]
    fn row_without_columns_is_ignored() {
        let mut a = Assembler::new();
        assert_eq!(a.push(row(&[Some("1")])), Step::Continue);
        a.push(Message::Done(0));
        assert!(a.finish(0).result.is_none());
    }

    #[test]
    fn stops_after_max_rows() {
        let mut a = Assembler::new();
        a.push(cols(&["n"]));
        for i in 0..MAX_ROWS {
            assert_eq!(a.push(row(&[Some(&i.to_string())])), Step::Continue);
        }
        assert_eq!(a.push(row(&[Some("extra")])), Step::Stop);
        let out = a.finish(0);
        let r = out.result.unwrap();
        assert_eq!(r.rows.len(), MAX_ROWS);
        assert!(r.truncated);
        let last = out.statements.last().unwrap();
        assert_eq!(last.rows, MAX_ROWS as u64);
        assert!(last.returned_columns);
    }
}
