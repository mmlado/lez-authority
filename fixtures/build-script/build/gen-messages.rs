use engine::Gated;

#[derive(Gated)]
enum Message {
    #[from_config]
    Credit,
}

fn main() {
    assert!(Message::Credit.only_from_config());
}
