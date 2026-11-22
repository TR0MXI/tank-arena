use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Message)]
struct Request(PathBuf);

#[derive(Resource)]
struct LoadJob {
    task: Task<io::Result<Playback>>,
    cancel: Arc<AtomicBool>,
    ready: Option<Playback>,
}

impl Drop for LoadJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub(super) fn install(app: &mut App) {
    app.add_message::<Request>()
        .add_systems(OnEnter(Page::Loading), (begin, loading))
        .add_systems(Update, poll.run_if(in_state(Page::Loading)))
        .add_systems(OnExit(Page::Loading), leave);
}

pub(in crate::game_ui) fn open(path: PathBuf) -> impl Bundle {
    observe(
        move |_: On<Activate>,
              mut requests: MessageWriter<Request>,
              mut next: ResMut<NextState<Page>>| {
            requests.write(Request(path.clone()));
            next.set(Page::Loading);
        },
    )
}

fn begin(mut c: Commands, mut requests: MessageReader<Request>, mut next: ResMut<NextState<Page>>) {
    let Some(Request(path)) = requests.read().last() else {
        next.set(Page::Library);
        return;
    };

    let path = path.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let task = AsyncComputeTaskPool::get()
        .spawn(async move { Playback::open_while(&path, || !token.load(Ordering::Relaxed)) });
    c.insert_resource(LoadJob {
        task,
        cancel,
        ready: None,
    });
}

fn poll(job: Option<ResMut<LoadJob>>, mut next: ResMut<NextState<Page>>, mut lib: ResMut<Library>) {
    if !matches!(*next, NextState::Unchanged) {
        return;
    }
    let Some(mut job) = job else {
        return;
    };

    if let Some(result) = check_ready(&mut job.task) {
        match result {
            Ok(p) => {
                job.ready = Some(p);
                next.set(Page::Replay);
            }
            Err(e) => {
                lib.notice = Some(locale::replay_error(&e));
                next.set(Page::Library);
            }
        }
    }
}

fn leave(world: &mut World) {
    if let Some(mut job) = world.remove_resource::<LoadJob>()
        && *world.resource::<State<Page>>().get() == Page::Replay
        && let Some(p) = job.ready.take()
    {
        world.insert_resource(ReplaySession::new(p));
    }
}

fn loading(mut c: Commands) {
    c.spawn((
        DespawnOnExit(Page::Loading),
        root(),
        BackgroundColor(paper()),
        children![(
            page_body(),
            children![(page_header(), children![headline(&tr("validating"), 30.)])]
        )],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;
    use std::time::Instant;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .init_state::<Page>()
            .init_resource::<Library>();
        install(&mut app);
        app.update();
        app
    }

    fn path(file: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file)
    }

    fn request(app: &mut App, path: PathBuf) {
        let w = app.world_mut();
        let entity = w.spawn(open(path)).id();
        w.trigger(Activate { entity });
    }

    fn wait_for(app: &mut App, target: Page) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while *app.world().resource::<State<Page>>().get() != target {
            assert!(Instant::now() < deadline, "did not enter {target:?}");
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(!app.world().contains_resource::<LoadJob>());
    }

    #[test]
    fn last_request_installs_validated_session_before_replay_entry() {
        let mut app = app();
        app.add_systems(OnEnter(Page::Replay), |session: Res<ReplaySession>| {
            assert_eq!(session.player.world().tick(), 0);
            assert!(session.player.length() > 0);
        });
        request(&mut app, path("not-found"));
        request(&mut app, path("first-duel.tankrec"));
        wait_for(&mut app, Page::Replay);

        assert!(app.world().contains_resource::<ReplaySession>());
        assert!(app.world().resource::<Library>().notice.is_none());
    }

    #[test]
    fn invalid_record_returns_to_library_without_session() {
        let mut app = app();
        let dir = tempfile::tempdir().unwrap();
        let invalid = dir.path().join("invalid.tankrec");
        std::fs::write(&invalid, b"not a replay").unwrap();
        request(&mut app, invalid);
        wait_for(&mut app, Page::Library);
        assert!(app.world().resource::<Library>().notice.is_some());
        assert!(!app.world().contains_resource::<ReplaySession>());
    }

    #[test]
    fn leaving_loading_cancels_job_and_does_not_install_its_result() {
        let mut app = app();
        request(&mut app, path("first-duel.tankrec"));
        app.update();
        assert_eq!(*app.world().resource::<State<Page>>().get(), Page::Loading);
        let token = app.world().resource::<LoadJob>().cancel.clone();
        app.world_mut()
            .resource_mut::<NextState<Page>>()
            .set(Page::Menu);
        wait_for(&mut app, Page::Menu);

        assert!(token.load(Ordering::Relaxed));
        app.update();

        assert!(!app.world().contains_resource::<ReplaySession>());
        assert!(app.world().resource::<Library>().notice.is_none());
    }
}
