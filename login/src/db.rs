//! MySQL access shaped like the C API calls the C++ makes: text-protocol queries whose rows are raw
//! byte strings (`MYSQL_ROW`) looked up by column name, and `ProcessQuery`'s ping/retry loop.
use crate::log::{put_log_file_list, WARN_MSG};
use crate::msg::{MAXALLOWEDQUERYERROR, MYSQL_ERROR_LOGFILE, QUERYERROR_LOGFILE};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, Row, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct DbCfg {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pwd: String,
}

impl DbCfg {
    pub fn opts(&self, db: &str) -> Opts {
        OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.pwd.clone()))
            .db_name(Some(db.to_string()))
            // libmysql's default charset: names travel as raw latin1 bytes both ways
            .setup(vec!["SET NAMES latin1"])
            .into()
    }
}

/// A stored result (`mysql_store_result`).
#[derive(Default, Debug)]
pub struct QRes {
    pub cols: Vec<String>,
    pub rows: Vec<Vec<Option<Vec<u8>>>>,
}

impl QRes {
    /// `myRow[f]` as a C string; NULL reads as "" (the C++ would crash on most NULLs).
    pub fn get(&self, row: usize, col: usize) -> &[u8] {
        self.rows.get(row).and_then(|r| r.get(col)).and_then(|v| v.as_deref()).unwrap_or(b"")
    }
    pub fn col(&self, name: &str) -> Option<usize> {
        self.cols.iter().position(|c| c == name)
    }
}

pub struct Db {
    conn: Option<Conn>,
    opts: Opts,
}

/// `MyAux_Get_Error`: logs and returns the error number.
fn log_err(e: &mysql_async::Error) -> u32 {
    let (code, text) = match e {
        mysql_async::Error::Server(s) => (s.code as u32, s.message.clone()),
        // client-side failures have no server errno; 2013 = CR_SERVER_LOST, 2003 = CR_CONN_HOST_ERROR
        mysql_async::Error::Io(_) => (2013, e.to_string()),
        _ => (2003, e.to_string()),
    };
    crate::log::log_to(&format!("(ERROR) MySql ERROR: [{code}] - {text}"), WARN_MSG, MYSQL_ERROR_LOGFILE);
    code
}

impl Db {
    /// `mysql_init` + `mysql_real_connect`; Err carries the logged errno.
    pub async fn connect(opts: Opts) -> Result<Db, u32> {
        match Conn::new(opts.clone()).await {
            Ok(c) => Ok(Db { conn: Some(c), opts }),
            Err(e) => Err(log_err(&e)),
        }
    }

    pub async fn ping(&mut self) -> bool {
        match &mut self.conn {
            Some(c) => c.ping().await.is_ok(),
            None => false,
        }
    }

    pub async fn close(&mut self) {
        if let Some(c) = self.conn.take() {
            let _ = c.disconnect().await;
        }
    }

    /// Rows changed by the last statement (`mysql_affected_rows`).
    pub fn affected_rows(&self) -> u64 {
        self.conn.as_ref().map_or(0, |c| c.affected_rows())
    }

    async fn run(&mut self, sql: &[u8]) -> Result<QRes, mysql_async::Error> {
        let conn = self.conn.as_mut().ok_or(mysql_async::Error::Other("not connected".into()))?;
        let res = conn.query_iter(sql).await?;
        let cols = res.columns().map(|c| c.iter().map(|c| c.name_str().into_owned()).collect()).unwrap_or_default();
        let rows: Vec<Row> = res.collect_and_drop().await?;
        let rows = rows
            .into_iter()
            .map(|r| {
                r.unwrap()
                    .into_iter()
                    .map(|v| match v {
                        Value::NULL => None,
                        Value::Bytes(b) => Some(b),
                        other => Some(other.as_sql(true).trim_matches('\'').as_bytes().to_vec()),
                    })
                    .collect()
            })
            .collect();
        Ok(QRes { cols, rows })
    }

    /// `ProcessQuery`: None is its -1. Quirks kept: after a ping failure it reconnects and gives up
    /// on this query (-1) without running it; the 10th attempt returns -1 even when it succeeded.
    // shortcut: C++ may also return mysql_query's nonzero code (ping fail after a failed query); folded into None.
    pub async fn q(&mut self, sql: &[u8]) -> Option<QRes> {
        let mut tries = 0;
        let mut out = None;
        loop {
            let err = if !self.ping().await {
                self.close().await;
                match Conn::new(self.opts.clone()).await {
                    Ok(c) => {
                        self.conn = Some(c);
                        None
                    }
                    Err(e) => Some(e),
                }
            } else {
                match self.run(sql).await {
                    Ok(r) => {
                        out = Some(r);
                        None
                    }
                    Err(e) => Some(e),
                }
            };
            tries += 1;
            let failed = match &err {
                Some(e) => log_err(e) != 0,
                None => false,
            };
            // faithful: Delay(1000) also pumps the window messages; here the whole server waits
            if failed {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
            if !failed || tries >= MAXALLOWEDQUERYERROR {
                break;
            }
        }
        if tries == MAXALLOWEDQUERYERROR {
            put_log_file_list(sql, Some(QUERYERROR_LOGFILE));
            return None;
        }
        out
    }

    /// Convenience for `format!`-built latin1 SQL.
    pub async fn qs(&mut self, sql: &str) -> Option<QRes> {
        self.q(&crate::util::to_l1(sql)).await
    }
}
