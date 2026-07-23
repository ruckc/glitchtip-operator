use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Encode everything except RFC 3986 unreserved characters, so passwords
/// containing @ : / % etc. survive inside a URL userinfo section.
const USERINFO: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn postgres_url(user: &str, password: &str, host: &str, port: &str, database: &str) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        utf8_percent_encode(user, USERINFO),
        utf8_percent_encode(password, USERINFO),
        host,
        port,
        utf8_percent_encode(database, USERINFO),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_credentials_pass_through() {
        assert_eq!(
            postgres_url("app", "s3cret", "db.ns.svc", "5432", "glitchtip"),
            "postgres://app:s3cret@db.ns.svc:5432/glitchtip"
        );
    }

    #[test]
    fn reserved_characters_are_encoded() {
        assert_eq!(
            postgres_url("app", "p@ss:w/rd%25", "db", "5432", "glitchtip"),
            "postgres://app:p%40ss%3Aw%2Frd%2525@db:5432/glitchtip"
        );
    }

    #[test]
    fn unreserved_characters_survive() {
        assert_eq!(
            postgres_url("a-b._~c", "x", "db", "5432", "d"),
            "postgres://a-b._~c:x@db:5432/d"
        );
    }
}
