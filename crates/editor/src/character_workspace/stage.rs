//! The workspace's viewport: an orbit camera with view presets, lighting
//! and backdrop presets, a floor, and drawing the preview World.

use axcenity_core::{ClassName, Color3, PartType, Value, Vector3, World};
use axcenity_render::Camera;
use axcenity_render::glam::Vec3;

/// The character is 5 studs tall, feet at the origin, facing -Z.
const HEIGHT: f32 = 5.0;

/// Where the camera looks from: turned `yaw` degrees round the character
/// (0 is in front), `pitch` degrees up, `distance` studs from `target`,
/// which panning moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for Orbit {
    fn default() -> Self {
        ViewPreset::FullBody.orbit()
    }
}

/// The view buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewPreset {
    FullBody,
    Face,
    Front,
    Side,
    Back,
}

impl ViewPreset {
    pub const ALL: [ViewPreset; 5] = [
        ViewPreset::FullBody,
        ViewPreset::Face,
        ViewPreset::Front,
        ViewPreset::Side,
        ViewPreset::Back,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewPreset::FullBody => "Full body",
            ViewPreset::Face => "Face",
            ViewPreset::Front => "Front",
            ViewPreset::Side => "Side",
            ViewPreset::Back => "Back",
        }
    }

    pub fn orbit(self) -> Orbit {
        let body = Vec3::new(0.0, HEIGHT * 0.5, 0.0);
        let full = |yaw: f32| Orbit {
            yaw,
            pitch: 6.0,
            distance: 13.5,
            target: body,
        };
        match self {
            ViewPreset::FullBody => Orbit {
                yaw: 25.0,
                ..full(0.0)
            },
            ViewPreset::Face => Orbit {
                yaw: 12.0,
                pitch: 2.0,
                distance: 3.2,
                target: Vec3::new(0.0, HEIGHT * 0.905, -0.05),
            },
            ViewPreset::Front => full(0.0),
            ViewPreset::Side => full(90.0),
            ViewPreset::Back => full(180.0),
        }
    }
}

impl Orbit {
    pub fn camera(&self) -> Camera {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        // The character faces -Z: yaw 0 looks at its front from -Z.
        let direction = Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            -yaw.cos() * pitch.cos(),
        );
        Camera {
            position: self.target + direction * self.distance,
            look_at: self.target,
            fov_y_degrees: 30.0,
        }
    }

    /// Turns with a drag (`dx`, `dy` in points).
    pub fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw = (self.yaw - dx * 0.4).rem_euclid(360.0);
        self.pitch = (self.pitch + dy * 0.3).clamp(-45.0, 80.0);
    }

    /// Zooms by a scroll (`scroll` points; up is closer).
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (1.0 - scroll * 0.0015)).clamp(1.2, 30.0);
    }

    /// Pans across the view by a drag (`dx`, `dy` in points), kept near
    /// the character.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let yaw = self.yaw.to_radians();
        let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let scale = self.distance * 0.0016;
        self.target += right * (-dx * scale) + Vec3::Y * (dy * scale);
        self.target = self.target.clamp(
            Vec3::new(-4.0, -0.5, -4.0),
            Vec3::new(4.0, HEIGHT + 1.0, 4.0),
        );
    }
}

/// Lighting presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lighting {
    #[default]
    Studio,
    Daylight,
    Sunset,
    Night,
}

impl Lighting {
    pub const ALL: [Lighting; 4] = [
        Lighting::Studio,
        Lighting::Daylight,
        Lighting::Sunset,
        Lighting::Night,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lighting::Studio => "Studio",
            Lighting::Daylight => "Daylight",
            Lighting::Sunset => "Sunset",
            Lighting::Night => "Night",
        }
    }

    /// The time of day it sets on the preview's Lighting.
    fn clock_time(self) -> f64 {
        match self {
            Lighting::Studio => 14.5,
            Lighting::Daylight => 12.5,
            Lighting::Sunset => 18.1,
            Lighting::Night => 21.5,
        }
    }
}

/// What's behind the character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Grey,
    Light,
    Dark,
    Sky,
}

impl Backdrop {
    pub const ALL: [Backdrop; 4] = [
        Backdrop::Grey,
        Backdrop::Light,
        Backdrop::Dark,
        Backdrop::Sky,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Backdrop::Grey => "Grey",
            Backdrop::Light => "Light",
            Backdrop::Dark => "Dark",
            Backdrop::Sky => "Sky",
        }
    }

    /// Top, horizon and ground colours (sRGB), or `None` for the sky.
    fn colors(self) -> Option<[[f32; 3]; 3]> {
        match self {
            Backdrop::Grey => Some([[0.42, 0.44, 0.47], [0.55, 0.56, 0.58], [0.30, 0.30, 0.31]]),
            Backdrop::Light => Some([[0.82, 0.83, 0.85], [0.90, 0.90, 0.91], [0.62, 0.62, 0.63]]),
            Backdrop::Dark => Some([[0.08, 0.09, 0.10], [0.15, 0.16, 0.17], [0.06, 0.06, 0.07]]),
            Backdrop::Sky => None,
        }
    }

    fn floor(self) -> [f32; 3] {
        match self {
            Backdrop::Grey | Backdrop::Sky => [0.46, 0.47, 0.49],
            Backdrop::Light => [0.78, 0.78, 0.79],
            Backdrop::Dark => [0.16, 0.16, 0.17],
        }
    }
}

/// A round floor under the character, for its shadow.
pub(super) fn add_floor(world: &mut World) {
    let workspace = world.workspace();
    let Ok(floor) = world.create(ClassName::Part) else {
        return;
    };
    let _ = world.set_name(floor, "Floor");
    for (name, value) in [
        ("Shape", Value::PartType(PartType::Cylinder)),
        ("Size", Value::Vector3(Vector3::new(0.4, 14.0, 14.0))),
        ("Position", Value::Vector3(Vector3::new(0.0, -0.2, 0.0))),
        ("Orientation", Value::Vector3(Vector3::new(0.0, 0.0, 90.0))),
        ("Anchored", Value::Bool(true)),
    ] {
        let _ = world.set_property(floor, name, value);
    }
    let _ = world.set_parent(floor, Some(workspace));
}

/// Sets the preview's time of day and floor colour for the presets.
pub(super) fn apply(world: &mut World, lighting: Lighting, backdrop: Backdrop) {
    let light = world.lighting();
    let _ = world.set_property(light, "ClockTime", Value::Number(lighting.clock_time()));
    let ambient = if lighting == Lighting::Night {
        0.35
    } else {
        0.5
    };
    let _ = world.set_property(
        light,
        "Ambient",
        Value::Color3(Color3::new(ambient, ambient, ambient * 1.05)),
    );
    let workspace = world.workspace();
    if let Some(floor) = world.find_first_child(workspace, "Floor") {
        let [r, g, b] = backdrop.floor();
        let _ = world.set_property(floor, "Color", Value::Color3(Color3::new(r, g, b)));
    }
}

/// Paints the backdrop's colours into the draw list's sky (the sky preset
/// keeps the procedural sky).
pub(super) fn backdrop(list: &mut axcenity_render::DrawList, backdrop: Backdrop) {
    if let Some([top, horizon, ground]) = backdrop.colors() {
        list.sky.top_color = top;
        list.sky.horizon_color = horizon;
        list.sky.ground_color = ground;
        list.sky.image = None;
        list.sky.atmosphere = None;
    }
    list.fog = Default::default();
}
