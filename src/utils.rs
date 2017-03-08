/// A very simple wrapper over the mpsc API, which allows us to only receive and send one Message
/// over the channel
// It has a very limited use, and we could have done without it with retrospective, but I don't
// think it's that bad either.

use std::sync::mpsc::{self, SyncSender, Receiver};

pub fn single_use_channel<T>() -> (SingleUseSender<T>, SingleUseReceiver<T>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let tx = SingleUseSender {
        inner: tx,
    };
    let rx = SingleUseReceiver {
        inner: rx,
    };
    (tx, rx)
}

pub struct SingleUseReceiver<T> {
    inner: Receiver<T>,
}

#[derive(Clone)]
/// even though this must be used only once,
/// we can still allow cloning: only the first send()
/// will be valid, all the others won't do anything.
///
/// Technically you will only want to call `send` once,
/// but in the case this is called multiple times because
/// of Clone, it is not an issue
pub struct SingleUseSender<T> {
    inner: SyncSender<T>,
}

impl<T> SingleUseReceiver<T> {
    pub fn recv(self) -> Result<T, mpsc::RecvError> {
        self.inner.recv()
    }
}

impl<T> SingleUseSender<T> {
    pub fn send(self, value: T) {
        let _r = self.inner.send(value);
    }
}
