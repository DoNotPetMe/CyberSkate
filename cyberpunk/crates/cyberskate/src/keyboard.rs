//! Skating without a controller: keys held down become an Xbox pad.
//!
//! The sticks do not jump between rest and full tilt: they travel there at
//! a stick's speed, so a quick down-then-up on the flick keys still passes
//! through the gesture Skate 3's flick-it recognises.

/// Keys that drive the virtual pad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub left_up: bool,
    pub left_down: bool,
    pub left_left: bool,
    pub left_right: bool,
    pub flick_up: bool,
    pub flick_down: bool,
    pub flick_left: bool,
    pub flick_right: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub left_bumper: bool,
    pub right_bumper: bool,
    pub left_trigger: bool,
    pub right_trigger: bool,
    pub start: bool,
    pub back: bool,
}

/// An Xbox pad state in XInput's layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u16,
    pub triggers: [u8; 2],
    pub left: [i16; 2],
    pub right: [i16; 2],
}

/// Full tilt per second a stick moves at.
const STICK_SPEED: f32 = 14.;

#[derive(Default)]
pub struct KeyboardPad {
    left: [f32; 2],
    right: [f32; 2],
}

fn direction(up: bool, down: bool, left: bool, right: bool) -> [f32; 2] {
    let x = f32::from(u8::from(right)) - f32::from(u8::from(left));
    let y = f32::from(u8::from(up)) - f32::from(u8::from(down));
    // A real stick's diagonal sits on the circle, not the square's corner.
    let scale = if x != 0. && y != 0. {
        std::f32::consts::FRAC_1_SQRT_2
    } else {
        1.
    };
    [x * scale, y * scale]
}

fn approach(stick: &mut [f32; 2], target: [f32; 2], step: f32) {
    let d = [target[0] - stick[0], target[1] - stick[1]];
    let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if length <= step {
        *stick = target;
    } else {
        stick[0] += d[0] / length * step;
        stick[1] += d[1] / length * step;
    }
}

fn axis(v: f32) -> i16 {
    (v.clamp(-1., 1.) * 32767.).round() as i16
}

impl KeyboardPad {
    /// The pad after `dt` seconds of `keys` held.
    pub fn update(&mut self, keys: Keys, dt: f32) -> Pad {
        let step = STICK_SPEED * dt.clamp(0., 0.1);
        approach(
            &mut self.left,
            direction(
                keys.left_up,
                keys.left_down,
                keys.left_left,
                keys.left_right,
            ),
            step,
        );
        approach(
            &mut self.right,
            direction(
                keys.flick_up,
                keys.flick_down,
                keys.flick_left,
                keys.flick_right,
            ),
            step,
        );
        let buttons = [
            (keys.start, 0x0010),
            (keys.back, 0x0020),
            (keys.left_bumper, 0x0100),
            (keys.right_bumper, 0x0200),
            (keys.a, 0x1000),
            (keys.b, 0x2000),
            (keys.x, 0x4000),
            (keys.y, 0x8000),
        ]
        .iter()
        .filter(|(held, _)| *held)
        .fold(0, |bits, (_, bit)| bits | bit);
        let trigger = |held: bool| if held { 255 } else { 0 };
        Pad {
            buttons,
            triggers: [trigger(keys.left_trigger), trigger(keys.right_trigger)],
            left: self.left.map(axis),
            right: self.right.map(axis),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flick_passes_through_the_middle() {
        let mut pad = KeyboardPad::default();
        let down = Keys {
            flick_down: true,
            ..Keys::default()
        };
        let up = Keys {
            flick_up: true,
            ..Keys::default()
        };
        for _ in 0..10 {
            pad.update(down, 1. / 60.);
        }
        assert_eq!(pad.update(down, 1. / 60.).right, [0, -32767]);
        let mut seen = Vec::new();
        for _ in 0..10 {
            seen.push(pad.update(up, 1. / 60.).right[1]);
        }
        assert!(seen.windows(2).all(|w| w[1] >= w[0]));
        assert!(seen.iter().any(|y| y.abs() < 12000), "{seen:?}");
        assert_eq!(*seen.last().unwrap(), 32767);
    }

    #[test]
    fn diagonals_sit_on_the_circle_and_buttons_map_to_xinput() {
        let mut pad = KeyboardPad::default();
        let keys = Keys {
            left_up: true,
            left_right: true,
            a: true,
            y: true,
            right_trigger: true,
            ..Keys::default()
        };
        let mut state = Pad::default();
        for _ in 0..30 {
            state = pad.update(keys, 1. / 60.);
        }
        assert_eq!(state.left, [23170, 23170]);
        assert_eq!(state.buttons, 0x1000 | 0x8000);
        assert_eq!(state.triggers, [0, 255]);
        let both = Keys {
            left_left: true,
            left_right: true,
            ..Keys::default()
        };
        for _ in 0..30 {
            state = pad.update(both, 1. / 60.);
        }
        assert_eq!(state.left, [0, 0]);
    }
}
