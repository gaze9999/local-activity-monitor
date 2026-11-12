pub fn database_path(value: &str) -> Option<&str> {
    if value.is_empty()
        || value.chars().count() > 512
        || value.chars().any(|ch| ch < ' ' || "$`*<>|".contains(ch))
    {
        return None;
    }
    if value.starts_with("file:") {
        return (!value.contains('@')).then(|| value.split('?').next().unwrap_or(value));
    }
    if value.contains('?')
        || value.split_once("://").is_some_and(|(scheme, _)| {
            !scheme.is_empty() && scheme.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
    {
        return None;
    }
    Some(value)
}
