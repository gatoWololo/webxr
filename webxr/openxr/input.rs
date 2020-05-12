use euclid::RigidTransform3D;
use openxr::d3d::D3D11;
use openxr::{
    self, Action, ActionSet, Binding, FrameState, Hand as HandEnum, HandJoint, HandTracker,
    Instance, Path, Posef, Quaternionf, Session, Space, SpaceLocationFlags, Vector3f,
};
use webxr_api::Finger;
use webxr_api::Hand;
use webxr_api::Handedness;
use webxr_api::Input;
use webxr_api::InputFrame;
use webxr_api::InputId;
use webxr_api::InputSource;
use webxr_api::JointFrame;
use webxr_api::Native;
use webxr_api::SelectEvent;
use webxr_api::TargetRayMode;
use webxr_api::Viewer;

/// Number of frames to wait with the menu gesture before
/// opening the menu.
const MENU_GESTURE_SUSTAIN_THRESHOLD: u8 = 60;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ClickState {
    Clicking,
    Done,
}

/// All the information on a single input frame
pub struct Frame {
    pub frame: InputFrame,
    pub select: Option<SelectEvent>,
    pub squeeze: Option<SelectEvent>,
    pub menu_selected: bool,
}

impl ClickState {
    fn update(
        &mut self,
        action: &Action<bool>,
        session: &Session<D3D11>,
        menu_selected: bool,
    ) -> (/* is_active */ bool, Option<SelectEvent>) {
        let click = action.state(session, Path::NULL).unwrap();

        let select_event = if click.is_active {
            match (click.current_state, *self) {
                (_, ClickState::Clicking) if menu_selected => {
                    *self = ClickState::Done;
                    // cancel the select, we're showing a menu
                    Some(SelectEvent::End)
                }
                (true, ClickState::Done) => {
                    *self = ClickState::Clicking;
                    Some(SelectEvent::Start)
                }
                (false, ClickState::Clicking) => {
                    *self = ClickState::Done;
                    Some(SelectEvent::Select)
                }
                _ => None,
            }
        } else if *self == ClickState::Clicking {
            *self = ClickState::Done;
            // cancel the select, we lost tracking
            Some(SelectEvent::End)
        } else {
            None
        };
        (click.is_active, select_event)
    }
}

const IDENTITY_POSE: Posef = Posef {
    orientation: Quaternionf {
        x: 0.,
        y: 0.,
        z: 0.,
        w: 1.,
    },
    position: Vector3f {
        x: 0.,
        y: 0.,
        z: 0.,
    },
};
pub struct OpenXRInput {
    id: InputId,
    action_aim_pose: Action<Posef>,
    action_aim_space: Space,
    action_grip_pose: Action<Posef>,
    action_grip_space: Space,
    action_click: Action<bool>,
    action_squeeze: Action<bool>,
    handedness: Handedness,
    click_state: ClickState,
    squeeze_state: ClickState,
    menu_gesture_sustain: u8,
    #[allow(unused)]
    hand_tracker: Option<HandTracker>,
    joints: Option<Hand<Space>>,
}

fn hand_str(h: Handedness) -> &'static str {
    match h {
        Handedness::Right => "right",
        Handedness::Left => "left",
        _ => panic!("We don't support unknown handedness in openxr"),
    }
}

impl OpenXRInput {
    pub fn new(
        id: InputId,
        handedness: Handedness,
        action_set: &ActionSet,
        session: &Session<D3D11>,
        needs_hands: bool,
    ) -> Self {
        let hand = hand_str(handedness);
        let action_aim_pose: Action<Posef> = action_set
            .create_action(
                &format!("{}_hand_aim", hand),
                &format!("{} hand aim", hand),
                &[],
            )
            .unwrap();
        let action_aim_space = action_aim_pose
            .create_space(session.clone(), Path::NULL, IDENTITY_POSE)
            .unwrap();
        let action_grip_pose: Action<Posef> = action_set
            .create_action(
                &format!("{}_hand_grip", hand),
                &format!("{} hand grip", hand),
                &[],
            )
            .unwrap();
        let action_grip_space = action_grip_pose
            .create_space(session.clone(), Path::NULL, IDENTITY_POSE)
            .unwrap();
        let action_click: Action<bool> = action_set
            .create_action(
                &format!("{}_hand_click", hand),
                &format!("{} hand click", hand),
                &[],
            )
            .unwrap();
        let action_squeeze: Action<bool> = action_set
            .create_action(
                &format!("{}_hand_squeeze", hand),
                &format!("{} hand squeeze", hand),
                &[],
            )
            .unwrap();

        let (hand_tracker, joints) = if needs_hands {
            let hand = match handedness {
                Handedness::Left => HandEnum::LEFT,
                Handedness::Right => HandEnum::RIGHT,
                _ => panic!("We don't support unknown handedness in openxr"),
            };
            let hand_tracker = session.create_hand_tracker(hand).unwrap();

            let joints = Hand {
                wrist: hand_tracker
                    .create_joint_space(HandJoint::WRIST, IDENTITY_POSE)
                    .ok(),
                thumb_metacarpal: hand_tracker
                    .create_joint_space(HandJoint::THUMB_METACARPAL, IDENTITY_POSE)
                    .ok(),
                thumb_phalanx_proximal: hand_tracker
                    .create_joint_space(HandJoint::THUMB_PROXIMAL, IDENTITY_POSE)
                    .ok(),
                thumb_phalanx_distal: hand_tracker
                    .create_joint_space(HandJoint::THUMB_DISTAL, IDENTITY_POSE)
                    .ok(),
                thumb_phalanx_tip: hand_tracker
                    .create_joint_space(HandJoint::THUMB_TIP, IDENTITY_POSE)
                    .ok(),
                index: Finger {
                    metacarpal: hand_tracker
                        .create_joint_space(HandJoint::INDEX_METACARPAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_proximal: hand_tracker
                        .create_joint_space(HandJoint::INDEX_PROXIMAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_intermediate: hand_tracker
                        .create_joint_space(HandJoint::INDEX_INTERMEDIATE, IDENTITY_POSE)
                        .ok(),
                    phalanx_distal: hand_tracker
                        .create_joint_space(HandJoint::INDEX_DISTAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_tip: hand_tracker
                        .create_joint_space(HandJoint::INDEX_TIP, IDENTITY_POSE)
                        .ok(),
                },
                middle: Finger {
                    metacarpal: hand_tracker
                        .create_joint_space(HandJoint::MIDDLE_METACARPAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_proximal: hand_tracker
                        .create_joint_space(HandJoint::MIDDLE_PROXIMAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_intermediate: hand_tracker
                        .create_joint_space(HandJoint::MIDDLE_INTERMEDIATE, IDENTITY_POSE)
                        .ok(),
                    phalanx_distal: hand_tracker
                        .create_joint_space(HandJoint::MIDDLE_DISTAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_tip: hand_tracker
                        .create_joint_space(HandJoint::MIDDLE_TIP, IDENTITY_POSE)
                        .ok(),
                },
                ring: Finger {
                    metacarpal: hand_tracker
                        .create_joint_space(HandJoint::RING_METACARPAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_proximal: hand_tracker
                        .create_joint_space(HandJoint::RING_PROXIMAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_intermediate: hand_tracker
                        .create_joint_space(HandJoint::RING_INTERMEDIATE, IDENTITY_POSE)
                        .ok(),
                    phalanx_distal: hand_tracker
                        .create_joint_space(HandJoint::RING_DISTAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_tip: hand_tracker
                        .create_joint_space(HandJoint::RING_TIP, IDENTITY_POSE)
                        .ok(),
                },
                little: Finger {
                    metacarpal: hand_tracker
                        .create_joint_space(HandJoint::LITTLE_METACARPAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_proximal: hand_tracker
                        .create_joint_space(HandJoint::LITTLE_PROXIMAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_intermediate: hand_tracker
                        .create_joint_space(HandJoint::LITTLE_INTERMEDIATE, IDENTITY_POSE)
                        .ok(),
                    phalanx_distal: hand_tracker
                        .create_joint_space(HandJoint::LITTLE_DISTAL, IDENTITY_POSE)
                        .ok(),
                    phalanx_tip: hand_tracker
                        .create_joint_space(HandJoint::LITTLE_TIP, IDENTITY_POSE)
                        .ok(),
                },
            };
            (Some(hand_tracker), Some(joints))
        } else {
            (None, None)
        };

        Self {
            id,
            action_aim_pose,
            action_aim_space,
            action_grip_pose,
            action_grip_space,
            action_click,
            action_squeeze,
            handedness,
            click_state: ClickState::Done,
            squeeze_state: ClickState::Done,
            menu_gesture_sustain: 0,
            hand_tracker,
            joints,
        }
    }

    pub fn setup_inputs(
        instance: &Instance,
        session: &Session<D3D11>,
        needs_hands: bool,
    ) -> (ActionSet, Self, Self) {
        let action_set = instance.create_action_set("hands", "Hands", 0).unwrap();
        let right_hand = OpenXRInput::new(
            InputId(0),
            Handedness::Right,
            &action_set,
            &session,
            needs_hands,
        );
        let left_hand = OpenXRInput::new(
            InputId(1),
            Handedness::Left,
            &action_set,
            &session,
            needs_hands,
        );

        let mut bindings =
            right_hand.get_bindings(instance, "trigger/value", Some("squeeze/click"));
        bindings.extend(
            left_hand
                .get_bindings(instance, "trigger/value", Some("squeeze/click"))
                .into_iter(),
        );
        let path_controller = instance
            .string_to_path("/interaction_profiles/microsoft/motion_controller")
            .unwrap();
        instance
            .suggest_interaction_profile_bindings(path_controller, &bindings)
            .unwrap();

        let mut bindings = right_hand.get_bindings(instance, "select/click", None);
        bindings.extend(
            left_hand
                .get_bindings(instance, "select/click", None)
                .into_iter(),
        );
        let path_controller = instance
            .string_to_path("/interaction_profiles/khr/simple_controller")
            .unwrap();
        instance
            .suggest_interaction_profile_bindings(path_controller, &bindings)
            .unwrap();
        session.attach_action_sets(&[&action_set]).unwrap();

        (action_set, right_hand, left_hand)
    }

    fn get_bindings(
        &self,
        instance: &Instance,
        select_name: &str,
        squeeze_name: Option<&str>,
    ) -> Vec<Binding> {
        let hand = hand_str(self.handedness);
        let path_aim_pose = instance
            .string_to_path(&format!("/user/hand/{}/input/aim/pose", hand))
            .unwrap();
        let binding_aim_pose = Binding::new(&self.action_aim_pose, path_aim_pose);
        let path_grip_pose = instance
            .string_to_path(&format!("/user/hand/{}/input/grip/pose", hand))
            .unwrap();
        let binding_grip_pose = Binding::new(&self.action_grip_pose, path_grip_pose);
        let path_click = instance
            .string_to_path(&format!("/user/hand/{}/input/{}", hand, select_name))
            .unwrap();
        let binding_click = Binding::new(&self.action_click, path_click);

        let mut ret = vec![binding_aim_pose, binding_grip_pose, binding_click];
        if let Some(squeeze_name) = squeeze_name {
            let path_squeeze = instance
                .string_to_path(&format!("/user/hand/{}/input/{}", hand, squeeze_name))
                .unwrap();
            let binding_squeeze = Binding::new(&self.action_squeeze, path_squeeze);
            ret.push(binding_squeeze);
        }
        ret
    }

    pub fn frame(
        &mut self,
        session: &Session<D3D11>,
        frame_state: &FrameState,
        base_space: &Space,
        viewer: &RigidTransform3D<f32, Viewer, Native>,
    ) -> Frame {
        use euclid::Vector3D;
        let target_ray_origin = pose_for(&self.action_aim_space, frame_state, base_space);

        let grip_origin = pose_for(&self.action_grip_space, frame_state, base_space);

        let mut menu_selected = false;
        // Check if the palm is facing up. This is our "menu" gesture.
        if let Some(grip_origin) = grip_origin {
            // The X axis of the grip is perpendicular to the palm, however its
            // direction is the opposite for each hand
            //
            // We obtain a unit vector pointing out of the palm
            let x_dir = if let Handedness::Left = self.handedness {
                1.0
            } else {
                -1.0
            };
            // Rotate it by the grip to obtain the desired vector
            let grip_x = grip_origin
                .rotation
                .transform_vector3d(Vector3D::new(x_dir, 0.0, 0.0));
            let gaze = viewer
                .rotation
                .transform_vector3d(Vector3D::new(0., 0., 1.));

            // If the angle is close enough to 0, its cosine will be
            // close to 1
            // check if the user's gaze is parallel to the palm
            if gaze.dot(grip_x) > 0.95 {
                let input_relative = (viewer.translation - grip_origin.translation).normalize();
                // if so, check if the user is actually looking at the palm
                if gaze.dot(input_relative) > 0.95 {
                    self.menu_gesture_sustain += 1;
                    if self.menu_gesture_sustain > MENU_GESTURE_SUSTAIN_THRESHOLD {
                        menu_selected = true;
                        self.menu_gesture_sustain = 0;
                    }
                } else {
                    self.menu_gesture_sustain = 0
                }
            } else {
                self.menu_gesture_sustain = 0;
            }
        } else {
            self.menu_gesture_sustain = 0;
        }

        let click = self.action_click.state(session, Path::NULL).unwrap();
        let squeeze = self.action_squeeze.state(session, Path::NULL).unwrap();

        let (click_is_active, click_event) =
            self.click_state
                .update(&self.action_click, session, menu_selected);
        let (squeeze_is_active, squeeze_event) =
            self.squeeze_state
                .update(&self.action_squeeze, session, menu_selected);

        let hand = target_ray_origin
            .and_then(|_origin| self.joints.as_ref())
            .map(|joints| {
                Box::new(joints.map(|j, _| {
                    j.as_ref()
                        .and_then(|j| joint_for(j, frame_state, base_space))
                }))
            });

        let input_frame = InputFrame {
            target_ray_origin,
            id: self.id,
            pressed: click_is_active && click.current_state,
            squeezed: squeeze_is_active && squeeze.current_state,
            grip_origin,
            hand,
        };

        Frame {
            frame: input_frame,
            select: click_event,
            squeeze: squeeze_event,
            menu_selected,
        }
    }

    pub fn input_source(&self) -> InputSource {
        InputSource {
            handedness: self.handedness,
            id: self.id,
            target_ray_mode: TargetRayMode::TrackedPointer,
            supports_grip: true,
            // XXXManishearth update with whatever we decide
            // in https://github.com/immersive-web/webxr-input-profiles/issues/105
            profiles: vec!["generic-hand".into()],
            hand_support: self
                .joints
                .as_ref()
                .map(|h| h.map(|j, _| j.as_ref().map(|_| ()))),
        }
    }
}

fn pose_for(
    action_space: &Space,
    frame_state: &FrameState,
    base_space: &Space,
) -> Option<RigidTransform3D<f32, Input, Native>> {
    let location = action_space
        .locate(base_space, frame_state.predicted_display_time)
        .unwrap();
    let pose_valid = location
        .location_flags
        .intersects(SpaceLocationFlags::POSITION_VALID | SpaceLocationFlags::ORIENTATION_VALID);
    if pose_valid {
        Some(super::transform(&location.pose))
    } else {
        None
    }
}

fn joint_for(
    joint_space: &Space,
    frame_state: &FrameState,
    base_space: &Space,
) -> Option<JointFrame> {
    let (location, radius) = joint_space
        .locate_radius(base_space, frame_state.predicted_display_time)
        .unwrap();
    let pose_valid = location
        .location_flags
        .intersects(SpaceLocationFlags::POSITION_VALID | SpaceLocationFlags::ORIENTATION_VALID);
    if pose_valid {
        Some(JointFrame {
            pose: super::transform(&location.pose),
            radius,
        })
    } else {
        None
    }
}
