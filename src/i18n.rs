/// Locale support. English is the source language; catalogs live in
/// `locales/`.  Set `LANG`/`LC_ALL` or `UUR_LANG` to switch.  The
/// `i18n!` macro itself lives at the crate root.
pub fn init() {
    let requested = std::env::var("UUR_LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".into());
    let code = requested.split('.').next().unwrap_or("en").to_string();
    let best = available()
        .into_iter()
        .find(|available| code.starts_with(available.as_str()))
        .unwrap_or_else(|| "en".into());
    rust_i18n::set_locale(&best);
}

fn available() -> Vec<String> {
    vec!["en".into(), "zh-CN".into()]
}
