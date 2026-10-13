use super::{Action, ActionExecutor, KeyboardSender, ProcessRunner};
use crate::keyboard::Chord;
use std::{cell::Cell, io, process::Child, rc::Rc};

struct Launcher {
    launched: Rc<Cell<bool>>,
}

impl ProcessRunner for Launcher {
    fn run(&mut self, _argv: &[String]) -> io::Result<()> {
        Err(io::Error::other("application must not use blocking run"))
    }

    fn launch(&mut self, argv: &[String]) -> io::Result<Option<Child>> {
        assert_eq!(argv, ["/usr/bin/obs", "--minimize-to-tray"]);
        self.launched.set(true);
        Ok(None)
    }
}

struct NoKeyboard;

impl KeyboardSender for NoKeyboard {
    fn send(&mut self, _chord: &Chord) -> io::Result<()> {
        Err(io::Error::other("launch must not send keyboard events"))
    }
}

#[test]
fn application_actions_use_the_nonblocking_launch_boundary() {
    let launched = Rc::new(Cell::new(false));
    let mut executor = ActionExecutor::new(
        Launcher {
            launched: launched.clone(),
        },
        NoKeyboard,
    );

    executor
        .execute(&Action::Launch {
            executable: "/usr/bin/obs".into(),
            args: vec!["--minimize-to-tray".into()],
        })
        .unwrap();

    assert!(launched.get());
    assert!(executor.poll_errors().is_empty());
}
