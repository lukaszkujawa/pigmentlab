use super::{Change, Film};
use crate::paint::Mark;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

enum Job {
    Lay(Vec<Mark>),
    Settle { dt: f32, most: usize },
    Forget,
    Relief(Vec<f32>),
    Unsettled,
}

enum Reply {
    Changes(Vec<Change>),
    Unsettled(f32),
}

pub struct FilmWorker {
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
    marks: Vec<Mark>,
    owed: f32,
    settling: bool,
    held: Option<Vec<Change>>,
}

impl FilmWorker {
    pub fn spawn(mut film: Film) -> Self {
        let (jobs, inbox) = channel();
        let (outbox, replies) = channel();
        std::thread::Builder::new()
            .name("film".into())
            .spawn(move || {
                for job in inbox {
                    let reply = match job {
                        Job::Lay(marks) => {
                            film.lay(&marks);
                            continue;
                        }
                        Job::Settle { dt, most } => Reply::Changes(film.settle(dt, most)),
                        Job::Forget => {
                            film.forget();
                            continue;
                        }
                        Job::Relief(relief) => {
                            film.set_relief(relief);
                            continue;
                        }
                        Job::Unsettled => Reply::Unsettled(film.unsettled()),
                    };
                    if outbox.send(reply).is_err() {
                        return;
                    }
                }
            })
            .expect("the film thread starts");
        Self { jobs, replies, marks: Vec::new(), owed: 0.0, settling: false, held: None }
    }

    pub fn lay(&mut self, marks: &[Mark]) {
        self.marks.extend_from_slice(marks);
    }

    pub fn settle(&mut self, dt: f32, most: usize, wait: bool) -> Option<Vec<Change>> {
        self.owed += dt;
        if wait && !self.settling {
            self.dispatch(most);
        }
        let changes = match (self.settling, wait) {
            (false, _) => None,
            (true, true) => Some(self.collect()),
            (true, false) => self.try_collect(),
        };
        if !wait && !self.settling {
            self.dispatch(most);
        }
        changes
    }

    pub fn forget(&mut self) {
        if self.settling {
            self.collect();
        }
        self.marks.clear();
        self.owed = 0.0;
        self.send(Job::Forget);
    }

    pub fn set_relief(&mut self, relief: Vec<f32>) {
        self.send(Job::Relief(relief));
    }

    pub fn unsettled(&mut self) -> f32 {
        self.flush_marks();
        self.send(Job::Unsettled);
        loop {
            match self.receive() {
                Reply::Unsettled(unsettled) => return unsettled,
                Reply::Changes(changes) => self.held = Some(changes),
            }
        }
    }

    fn dispatch(&mut self, most: usize) {
        self.flush_marks();
        self.send(Job::Settle { dt: self.owed, most });
        self.owed = 0.0;
        self.settling = true;
    }

    fn flush_marks(&mut self) {
        if !self.marks.is_empty() {
            let marks = std::mem::take(&mut self.marks);
            self.send(Job::Lay(marks));
        }
    }

    fn collect(&mut self) -> Vec<Change> {
        self.settling = false;
        match self.held.take() {
            Some(changes) => changes,
            None => match self.receive() {
                Reply::Changes(changes) => changes,
                Reply::Unsettled(_) => unreachable!("unsettled replies are taken where they are asked for"),
            },
        }
    }

    fn try_collect(&mut self) -> Option<Vec<Change>> {
        if self.held.is_some() {
            return Some(self.collect());
        }
        match self.replies.try_recv() {
            Ok(Reply::Changes(changes)) => {
                self.settling = false;
                Some(changes)
            }
            Ok(Reply::Unsettled(_)) => unreachable!("unsettled replies are taken where they are asked for"),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => panic!("the film thread stopped"),
        }
    }

    fn send(&self, job: Job) {
        self.jobs.send(job).expect("the film thread is running");
    }

    fn receive(&self) -> Reply {
        self.replies.recv().expect("the film thread is running")
    }
}
