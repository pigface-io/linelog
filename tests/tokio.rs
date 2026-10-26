//! Records from tokio's worker threads, through a background sink: every
//! line whole, none lost, each task's lines in order.

use linelog::{Background, Capture, Format, Level, info};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tasks_on_worker_threads() {
    linelog::flush();
    let lines = Capture::new();
    linelog::init()
        .level(Level::Info)
        .format(Format::Json)
        .sink(Background::new(lines.clone()))
        .install()
        .unwrap();
    let tasks: Vec<_> = (0..64_u32)
        .map(|task| {
            tokio::spawn(async move {
                for i in 0..500_u32 {
                    info!(target: "app", "tick"; task, i);
                    if i % 50 == 0 {
                        tokio::task::yield_now().await;
                    }
                }
            })
        })
        .collect();
    for t in tasks {
        t.await.unwrap();
    }
    linelog::flush();
    let got = lines.lines();
    assert_eq!(got.len(), 64 * 500);
    for task in 0..64 {
        let key = format!(r#""task":{task},"i":"#);
        let seen: Vec<u32> = got
            .iter()
            .filter_map(|l| l.split_once(&key))
            .map(|(_, rest)| rest.trim_end_matches('}').parse().unwrap())
            .collect();
        assert_eq!(seen, (0..500).collect::<Vec<_>>(), "task {task}");
    }
}
