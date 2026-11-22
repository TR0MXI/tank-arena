rust_i18n::i18n!("locales", fallback = "en");

mod game_ui;

fn main() {
    game_ui::run();
}
