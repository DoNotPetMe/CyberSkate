//! The skate engine on its own threads.
//!
//! One thread owns the `Session` and steps it; a second turns scans into
//! prepared collision so a rebuild never stalls a tick. The game thread only
//! sends jobs and reads the latest published frame, so no call made from the
//! game ever waits on the simulation.
use crate::coords::{self, Frame};
use crate::scan::{Collision, Scan};
use bevy::math::Vec3;
use skate_host::bridge::{ControllerTransport, PreparedCollision, Session};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

/// Longest a rebuilt world waits for a grind to finish before it is
/// installed anyway.
const GRIND_DEFER: Duration = Duration::from_secs(2);
/// Longest activation waits for the scan around its position to be built.
const FIRST_COLLISION: Duration = Duration::from_secs(3);
/// Simulation time owed beyond this is dropped rather than caught up.
const MAX_BACKLOG: f32 = 0.15;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Loading,
    Ready,
    Active,
    Failed(String),
}

impl Status {
    pub fn describe(&self) -> String {
        match self {
            Status::Loading => "loading".into(),
            Status::Ready => "ready".into(),
            Status::Active => "active".into(),
            Status::Failed(e) => format!("error: {e}"),
        }
    }
}

/// The skater as last simulated, in Night City coordinates.
#[derive(Clone, Debug)]
pub struct SkateFrame {
    pub sequence: u64,
    pub tick: u64,
    /// The animated skater's root: on the ground between the feet, facing
    /// where the skater's body faces.
    pub skater: Frame,
    pub deck: Frame,
    pub head: Option<Vec3>,
    /// Skate 3's own camera and its vertical field of view in degrees.
    pub camera: Option<(Frame, f32)>,
    pub velocity: Vec3,
    pub state: String,
}

impl SkateFrame {
    /// Length of `to_floats`.
    pub const LEN: usize = 33;

    /// The frame as the mod reads it, all in Night City coordinates:
    ///
    /// | index | value |
    /// |---|---|
    /// | 0, 1 | sequence, simulation tick |
    /// | 2–4, 5–7, 8–10 | skater root position, forward, up |
    /// | 11–13, 14–17 | deck position, rotation quaternion x y z w (local x right, y nose, z up) |
    /// | 18–20, 21–23, 24, 25 | camera position, forward, vertical field of view in degrees, 1 when present |
    /// | 26–28 | velocity, metres per second |
    /// | 29–31, 32 | head position, 1 when present |
    pub fn to_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend([self.sequence as f32, self.tick as f32]);
        out.extend(self.skater.position.to_array());
        out.extend(self.skater.forward.to_array());
        out.extend(self.skater.up.to_array());
        out.extend(self.deck.position.to_array());
        out.extend(self.deck.rotation().to_array());
        let (camera, fov, has_camera) = match self.camera {
            Some((frame, fov)) => (frame, fov, 1.),
            None => (Frame::IDENTITY, 0., 0.),
        };
        out.extend(camera.position.to_array());
        out.extend(camera.forward.to_array());
        out.extend([fov, has_camera]);
        out.extend(self.velocity.to_array());
        out.extend(self.head.unwrap_or(Vec3::ZERO).to_array());
        out.push(if self.head.is_some() { 1. } else { 0. });
        debug_assert_eq!(out.len(), Self::LEN);
        out
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CollisionInfo {
    pub generation: u64,
    pub triangles: usize,
    pub rails: usize,
}

struct Shared {
    status: Status,
    frame: Option<SkateFrame>,
    controller: bool,
    collision: CollisionInfo,
    notice: Option<String>,
}

enum Job {
    Scan(Box<Scan>),
    Activate {
        epoch: u64,
        position: Vec3,
        forward: Vec3,
    },
    Step {
        epoch: u64,
        dt: f32,
    },
    Suspend,
}

type Built = (u64, Result<(PreparedCollision, CollisionInfo), String>);

pub struct Runtime {
    jobs: mpsc::Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    epoch: u64,
    root: PathBuf,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Runtime {
    /// Starts loading the converted Skate 3 data in `root` (the converter's
    /// `assets` folder). Loading takes seconds; poll `status`.
    pub fn start(root: &Path) -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared {
            status: Status::Loading,
            frame: None,
            controller: false,
            collision: CollisionInfo::default(),
            notice: None,
        }));
        let (jobs, receive) = mpsc::channel();
        let worker = Arc::clone(&shared);
        let assets = root.to_owned();
        std::thread::Builder::new()
            .name("cyberskate".into())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    simulate(&assets, receive, &worker)
                }));
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(e)) => e,
                    Err(panic) => panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_else(|| "the skate engine panicked".into()),
                };
                lock(&worker).status = Status::Failed(error);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            jobs,
            shared,
            epoch: 0,
            root: root.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn status(&self) -> Status {
        lock(&self.shared).status.clone()
    }

    pub fn frame(&self) -> Option<SkateFrame> {
        lock(&self.shared).frame.clone()
    }

    pub fn controller(&self) -> bool {
        lock(&self.shared).controller
    }

    pub fn collision(&self) -> CollisionInfo {
        lock(&self.shared).collision
    }

    /// The latest message worth showing the player, once.
    pub fn take_notice(&self) -> Option<String> {
        lock(&self.shared).notice.take()
    }

    fn send(&self, job: Job) -> bool {
        self.jobs.send(job).is_ok()
    }

    /// Replaces the world the skater rides with `scan`, built off the
    /// simulation thread.
    pub fn submit(&self, scan: Scan) -> bool {
        self.send(Job::Scan(Box::new(scan)))
    }

    /// Puts the skater on the board at `position`, rolling along `forward`.
    pub fn activate(&mut self, position: Vec3, forward: Vec3) -> bool {
        self.epoch += 1;
        lock(&self.shared).frame = None;
        self.send(Job::Activate {
            epoch: self.epoch,
            position,
            forward,
        })
    }

    /// Advances the skater by `dt` seconds of game time, reading the
    /// controller as it goes.
    pub fn step(&self, dt: f32) -> bool {
        if !dt.is_finite() || dt <= 0. {
            return true;
        }
        self.send(Job::Step {
            epoch: self.epoch,
            dt: dt.min(0.1),
        })
    }

    pub fn suspend(&mut self) -> bool {
        self.epoch += 1;
        self.send(Job::Suspend)
    }
}

fn placeholder() -> Vec<[[f32; 3]; 3]> {
    let p = |x: f32, y: f32| coords::to_skate(Vec3::new(x, y, -500.)).to_array();
    vec![
        [p(-2., -2.), p(2., -2.), p(2., 2.)],
        [p(-2., -2.), p(2., 2.), p(-2., 2.)],
    ]
}

fn simulate(root: &Path, jobs: mpsc::Receiver<Job>, shared: &Mutex<Shared>) -> Result<(), String> {
    let mut session = Session::new(root, placeholder(), vec![], [0.; 3], 0.)?;
    let builder = session.collision_builder();
    let (scans, scan_jobs) = mpsc::channel::<(u64, Box<Scan>)>();
    let (built_send, built) = mpsc::channel::<Built>();
    std::thread::Builder::new()
        .name("cyberskate-collision".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            while let Ok(mut latest) = scan_jobs.recv() {
                while let Ok(newer) = scan_jobs.try_recv() {
                    latest = newer;
                }
                let (generation, scan) = latest;
                let prepared = Collision::from_scan(&scan).and_then(|c| {
                    let info = CollisionInfo {
                        generation,
                        triangles: c.triangles.len(),
                        rails: c.rails.len(),
                    };
                    Ok((builder.build(c.triangles, c.rails)?, info))
                });
                if built_send.send((generation, prepared)).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    lock(shared).status = Status::Ready;

    let mut transport = ControllerTransport::default();
    let mut queue = VecDeque::new();
    let mut generation = 0;
    let mut installed = 0;
    let mut pending: Option<(u64, PreparedCollision, CollisionInfo)> = None;
    let mut deferred_since: Option<Instant> = None;
    let mut epoch = 0;
    let mut active = false;
    let mut accumulated = 0.;
    let mut sequence = 0;
    let mut state = String::new();

    loop {
        let job = match queue.pop_front() {
            Some(job) => job,
            None => match jobs.recv() {
                Ok(job) => job,
                Err(_) => return Ok(()),
            },
        };
        // Newly built worlds wait here until a tick may take them.
        while let Ok((built_generation, prepared)) = built.try_recv() {
            match prepared {
                Ok((prepared, info)) if built_generation > installed => {
                    pending = Some((built_generation, prepared, info));
                }
                Ok(_) => {}
                Err(e) => lock(shared).notice = Some(format!("Scan skipped: {e}")),
            }
        }
        match job {
            Job::Scan(scan) => {
                generation += 1;
                let _ = scans.send((generation, scan));
            }
            Job::Suspend => {
                active = false;
                accumulated = 0.;
                session.suspend_input();
                lock(shared).status = Status::Ready;
            }
            Job::Activate {
                epoch: new_epoch,
                position,
                forward,
            } => {
                epoch = new_epoch;
                active = false;
                accumulated = 0.;
                // The world around the new position is the scan sent just
                // before this activation; an older one may be anywhere.
                let wanted = generation;
                let deadline = Instant::now() + FIRST_COLLISION;
                while pending.as_ref().map_or(installed, |(g, _, _)| *g) < wanted {
                    let left = deadline.saturating_duration_since(Instant::now());
                    match built.recv_timeout(left) {
                        Ok((g, Ok((prepared, info)))) => {
                            if g > installed {
                                pending = Some((g, prepared, info));
                            }
                        }
                        Ok((g, Err(e))) => {
                            lock(shared).notice = Some(format!("Scan skipped: {e}"));
                            if g >= wanted {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                if let Some((g, prepared, info)) = pending.take() {
                    session.install_collision(prepared)?;
                    installed = g;
                    lock(shared).collision = info;
                }
                if installed == 0 || installed < wanted {
                    lock(shared).notice = Some("The ground here could not be scanned.".into());
                    continue;
                }
                let heading = coords::heading(forward).unwrap_or(0.);
                let pose = session.activate(coords::to_skate(position).to_array(), heading)?;
                sequence += 1;
                state = pose.state.clone();
                publish(shared, &pose, sequence)?;
                active = true;
                lock(shared).status = Status::Active;
            }
            Job::Step {
                epoch: step_epoch,
                mut dt,
            } => {
                if step_epoch != epoch || !active {
                    continue;
                }
                // Steps that queued up while this thread was busy are one
                // longer step; anything else keeps its order.
                while let Ok(next) = jobs.try_recv() {
                    match next {
                        Job::Step { epoch: e, dt: more } if e == epoch && queue.is_empty() => {
                            dt += more
                        }
                        other => queue.push_back(other),
                    }
                }
                if let Some((g, _, _)) = &pending {
                    let grinding = state.starts_with("Grind");
                    let waited = deferred_since.get_or_insert_with(Instant::now).elapsed();
                    if !grinding || waited > GRIND_DEFER {
                        let g = *g;
                        let (_, prepared, info) = pending.take().unwrap();
                        session.install_collision(prepared)?;
                        installed = g;
                        deferred_since = None;
                        lock(shared).collision = info;
                    }
                }
                let input = transport.poll();
                let controller = input.controller().is_some();
                session.collect(input, dt);
                accumulated = (accumulated + dt).min(MAX_BACKLOG);
                let mut advanced = false;
                while accumulated >= session.period() {
                    accumulated -= session.period();
                    session.advance()?;
                    advanced = true;
                }
                lock(shared).controller = controller;
                if advanced {
                    let pose = session.pose();
                    sequence += 1;
                    state = pose.state.clone();
                    publish(shared, &pose, sequence)?;
                }
            }
        }
    }
}

fn publish(
    shared: &Mutex<Shared>,
    pose: &skate_host::bridge::Pose,
    sequence: u64,
) -> Result<(), String> {
    if !pose.root.is_finite() || !pose.deck.is_finite() || pose.bones.iter().any(|b| !b.is_finite())
    {
        return Err("the skate engine published a non-finite pose".into());
    }
    let head = pose
        .names
        .iter()
        .position(|n| n == "HEAD")
        .and_then(|i| pose.bones.get(i))
        .map(|m| coords::from_skate(m.w_axis.truncate()));
    let frame = SkateFrame {
        sequence,
        tick: pose.tick,
        skater: Frame::from_skate_matrix(pose.root),
        deck: Frame::from_skate_matrix(pose.deck),
        head,
        camera: pose
            .camera
            .map(|(position, basis, fov)| (Frame::from_skate_basis(position, basis), fov)),
        velocity: coords::from_skate(pose.velocity),
        state: pose.state.clone(),
    };
    lock(shared).frame = Some(frame);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_skate_data_fails_with_a_reason() {
        let root = std::env::temp_dir().join("cyberskate-no-data");
        let mut runtime = Runtime::start(&root).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            match runtime.status() {
                Status::Loading if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                other => break other,
            }
        };
        match status {
            Status::Failed(e) => assert!(e.contains("cyberskate-no-data"), "{e}"),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert!(runtime.frame().is_none());
        // A dead engine takes no more work, without panicking the caller.
        assert!(!runtime.activate(Vec3::ZERO, Vec3::Y));
        assert!(!runtime.step(0.016));
    }
}
