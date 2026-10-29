//! 取得物のバージョン文字列の検証。URLやファイルパスへ埋め込むため、安全な文字だけを許す。
use crate::utils::error::AppError;

const MAX_LEN: usize = 64;

/// `0.17.1`・`v1.11.1`・`main`のような識別子か。`/`や`..`を含めない(取得元ホストやパスを変えさせない)。
pub fn validate(value: &str) -> Result<(), AppError> {
    let ok = !value.is_empty()
        && value.len() <= MAX_LEN
        && !value.starts_with('.')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(()) } else { Err(AppError::InvalidVersion(value.chars().take(MAX_LEN).collect())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_release_names() {
        for v in ["0.17.1", "v1.11.1", "main", "1.23.2", "a1b2c3d"] {
            assert!(validate(v).is_ok(), "{v}");
        }
    }

    #[test]
    fn rejects_path_and_url_characters() {
        for v in ["", ".", "..", "../x", "a/b", "a b", "a?b", "a:b", "あ", &"1".repeat(65)] {
            assert!(validate(v).is_err(), "{v}");
        }
    }
}
