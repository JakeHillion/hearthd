//! A small SNMPv1 client over [`snmp2`], with the timeouts and retries that
//! crate leaves to its caller.
//!
//! Every attempt opens a fresh session. `snmp2` rejects a late reply to an
//! abandoned request rather than skipping it, so reusing a socket after a
//! timeout would fail the next request too.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use snmp2::AsyncSession;
use snmp2::Oid;
use snmp2::Value;

/// How many times a request is sent before giving up.
const ATTEMPTS: u32 = 3;

/// A value read from the agent, owned so it can outlive the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Integer(i64),
    Text(String),
    /// The agent has no such object, or answered with a type hearthd does
    /// not read.
    Missing,
}

impl Reading {
    pub fn integer(&self) -> Option<i64> {
        match self {
            Reading::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Reading::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl From<&Value<'_>> for Reading {
    fn from(value: &Value<'_>) -> Self {
        match value {
            Value::Integer(i) => Reading::Integer(*i),
            Value::Unsigned32(u) | Value::Counter32(u) => Reading::Integer(i64::from(*u)),
            Value::OctetString(bytes) => Reading::Text(String::from_utf8_lossy(bytes).into_owned()),
            _ => Reading::Missing,
        }
    }
}

/// One SNMP agent.
#[derive(Debug, Clone)]
pub struct Agent {
    host: String,
    port: u16,
    timeout: Duration,
}

impl Agent {
    pub fn new(host: String, port: u16, timeout: Duration) -> Self {
        Self {
            host,
            port,
            timeout,
        }
    }

    /// Read `oids` in one request, in order.
    pub async fn get(&self, community: &str, oids: &[Vec<u64>]) -> Result<Vec<Reading>> {
        let oids = oids
            .iter()
            .map(|oid| to_oid(oid))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<&Oid<'_>> = oids.iter().collect();

        let mut last_error = None;
        for _ in 0..ATTEMPTS {
            match self.get_once(community, &refs).await {
                Ok(readings) => return Ok(readings),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.expect("at least one attempt"))
    }

    async fn get_once(&self, community: &str, oids: &[&Oid<'_>]) -> Result<Vec<Reading>> {
        let mut session = self.session(community).await?;
        let pdu = tokio::time::timeout(self.timeout, session.get_many(oids))
            .await
            .context("timed out")?
            .context("GET failed")?;
        check_status(pdu.error_status, pdu.error_index)?;

        let readings: Vec<Reading> = pdu
            .varbinds
            .map(|(_, value)| Reading::from(&value))
            .collect();
        if readings.len() != oids.len() {
            anyhow::bail!("asked for {} objects, got {}", oids.len(), readings.len());
        }
        Ok(readings)
    }

    /// Set `oid` to the integer `value`.
    ///
    /// Retried like a read, so it must only be used for writes that are safe
    /// to repeat.
    pub async fn set_integer(&self, community: &str, oid: &[u64], value: i64) -> Result<()> {
        let oid = to_oid(oid)?;

        let mut last_error = None;
        for _ in 0..ATTEMPTS {
            match self.set_once(community, &oid, value).await {
                Ok(()) => return Ok(()),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.expect("at least one attempt"))
    }

    async fn set_once(&self, community: &str, oid: &Oid<'_>, value: i64) -> Result<()> {
        let mut session = self.session(community).await?;
        let pdu = tokio::time::timeout(self.timeout, session.set(&[(oid, Value::Integer(value))]))
            .await
            .context("timed out")?
            .context("SET failed")?;
        check_status(pdu.error_status, pdu.error_index)
    }

    async fn session(&self, community: &str) -> Result<AsyncSession> {
        AsyncSession::new_v1((self.host.as_str(), self.port), community.as_bytes(), 0)
            .await
            .with_context(|| format!("failed to open an SNMP session to {}", self.host))
    }
}

fn to_oid(oid: &[u64]) -> Result<Oid<'static>> {
    Oid::from(oid).map_err(|e| anyhow::anyhow!("invalid OID {oid:?}: {e:?}"))
}

/// Turn a response's SNMPv1 error status into an error.
fn check_status(status: u32, index: u32) -> Result<()> {
    let name = match status {
        0 => return Ok(()),
        1 => "tooBig",
        2 => "noSuchName",
        3 => "badValue",
        4 => "readOnly",
        _ => "genErr",
    };
    anyhow::bail!("agent answered {name} (status {status}) for object {index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauges_and_integers_read_as_integers() {
        assert_eq!(Reading::from(&Value::Integer(-3)), Reading::Integer(-3));
        assert_eq!(Reading::from(&Value::Unsigned32(10)), Reading::Integer(10));
        assert_eq!(
            Reading::from(&Value::OctetString(b"stinger.pop")),
            Reading::Text("stinger.pop".to_string())
        );
        assert_eq!(Reading::from(&Value::NoSuchInstance), Reading::Missing);
    }

    #[test]
    fn a_zero_status_is_success_and_anything_else_names_the_object() {
        assert!(check_status(0, 0).is_ok());
        let err = check_status(2, 3).unwrap_err().to_string();
        assert!(err.contains("noSuchName"), "{err}");
        assert!(err.contains("object 3"), "{err}");
    }
}
