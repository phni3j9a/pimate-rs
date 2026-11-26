//! Shell-style folder completion for a path on the selected Host.
pub fn split_folder_path(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    if !text.starts_with('/') && !text.starts_with("~/") {
        return None;
    }
    Some(text.split_at(text.rfind('/')? + 1))
}

pub fn match_folders(names: Vec<String>, prefix: &str) -> Vec<String> {
    let prefix = prefix.to_lowercase();
    names
        .into_iter()
        .filter(|name| {
            (!name.starts_with('.') || prefix.starts_with('.'))
                && name.to_lowercase().starts_with(&prefix)
        })
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_absolute_or_home_paths() {
        assert_eq!(split_folder_path("foo/bar"), None);
        assert_eq!(split_folder_path("~"), None);
        assert_eq!(split_folder_path(" ~/code/S"), Some(("~/code/", "S")));
        assert_eq!(split_folder_path("/"), Some(("/", "")));
        let names = || {
            [".git", "SRC", "a", "b", "c", "d", "e", "f", "g", "h"]
                .map(String::from)
                .to_vec()
        };
        assert_eq!(
            match_folders(names(), ""),
            ["SRC", "a", "b", "c", "d", "e", "f", "g"]
        );
        assert_eq!(match_folders(names(), "s"), ["SRC"]);
        assert_eq!(match_folders(names(), ".G"), [".git"]);
    }
}
