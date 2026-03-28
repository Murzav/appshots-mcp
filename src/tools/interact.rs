use std::time::Duration;

use serde::Serialize;
use tokio::time::sleep;

use crate::error::AppShotsError;

#[derive(Debug, Serialize)]
pub(crate) struct InteractResult {
    pub action: String,
    pub success: bool,
}

/// Interact with iOS Simulator via CGEvent mouse simulation.
///
/// - `action`: `"scroll"` or `"tap"`
/// - `x`, `y`: screen coordinates (required for tap, optional for scroll)
/// - `dx`, `dy`: scroll delta in pixels (positive dy = scroll content down)
/// - `delay_ms`: settle time after action
pub(crate) async fn handle_interact_simulator(
    action: &str,
    x: Option<f64>,
    y: Option<f64>,
    dx: Option<f64>,
    dy: Option<f64>,
    delay_ms: u64,
) -> Result<InteractResult, AppShotsError> {
    match action {
        "scroll" => {
            let scroll_dx = dx.unwrap_or(0.0);
            let scroll_dy = dy.unwrap_or(0.0);
            // Default start position: center-ish of a typical simulator window
            let start_x = x.unwrap_or(200.0);
            let start_y = y.unwrap_or(400.0);
            execute_scroll(scroll_dx, scroll_dy, start_x, start_y)?;
        }
        "tap" => {
            let tap_x = x.ok_or_else(|| AppShotsError::InteractionFailed {
                action: "tap".into(),
                detail: "x coordinate is required for tap".into(),
            })?;
            let tap_y = y.ok_or_else(|| AppShotsError::InteractionFailed {
                action: "tap".into(),
                detail: "y coordinate is required for tap".into(),
            })?;
            execute_tap(tap_x, tap_y)?;
        }
        other => {
            return Err(AppShotsError::InteractionFailed {
                action: other.into(),
                detail: format!("unknown action: {other}, expected scroll or tap"),
            });
        }
    }

    // Wait for UI to settle
    sleep(Duration::from_millis(delay_ms)).await;

    Ok(InteractResult {
        action: action.into(),
        success: true,
    })
}

/// Simulate a scroll by performing a mouse drag sequence.
///
/// The user's `dy` means "scroll content down by dy pixels", which translates
/// to dragging upward (negative screen Y direction). We negate dy for drag.
fn execute_scroll(dx: f64, dy: f64, start_x: f64, start_y: f64) -> Result<(), AppShotsError> {
    use core_graphics::event::{CGEvent, CGEventType, CGMouseButton};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGPoint;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| {
        AppShotsError::InteractionFailed {
            action: "scroll".into(),
            detail: "failed to create CGEventSource — check macOS Accessibility permission".into(),
        }
    })?;

    let start = CGPoint::new(start_x, start_y);
    let steps = 25u32;
    // Negate dy: "scroll content down" = drag cursor upward
    let drag_dx = dx / steps as f64;
    let drag_dy = -dy / steps as f64;

    // Mouse down at start position
    let down = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::LeftMouseDown,
        start,
        CGMouseButton::Left,
    )
    .map_err(|_| AppShotsError::InteractionFailed {
        action: "scroll".into(),
        detail: "failed to create mouse down event".into(),
    })?;
    down.post(core_graphics::event::CGEventTapLocation::HID);
    std::thread::sleep(Duration::from_millis(50));

    // Incremental drag steps (~60fps timing)
    for i in 1..=steps {
        let point = CGPoint::new(start_x + drag_dx * i as f64, start_y + drag_dy * i as f64);
        let drag = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseDragged,
            point,
            CGMouseButton::Left,
        )
        .map_err(|_| AppShotsError::InteractionFailed {
            action: "scroll".into(),
            detail: "failed to create drag event".into(),
        })?;
        drag.post(core_graphics::event::CGEventTapLocation::HID);
        std::thread::sleep(Duration::from_millis(16));
    }

    // Mouse up at end position
    let end = CGPoint::new(start_x + dx, start_y - dy);
    let up = CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, end, CGMouseButton::Left)
        .map_err(|_| AppShotsError::InteractionFailed {
        action: "scroll".into(),
        detail: "failed to create mouse up event".into(),
    })?;
    up.post(core_graphics::event::CGEventTapLocation::HID);

    Ok(())
}

/// Simulate a tap at the given screen coordinates.
fn execute_tap(x: f64, y: f64) -> Result<(), AppShotsError> {
    use core_graphics::event::{CGEvent, CGEventType, CGMouseButton};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGPoint;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| {
        AppShotsError::InteractionFailed {
            action: "tap".into(),
            detail: "failed to create CGEventSource — check macOS Accessibility permission".into(),
        }
    })?;

    let point = CGPoint::new(x, y);

    let down = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::LeftMouseDown,
        point,
        CGMouseButton::Left,
    )
    .map_err(|_| AppShotsError::InteractionFailed {
        action: "tap".into(),
        detail: "failed to create mouse down event".into(),
    })?;
    down.post(core_graphics::event::CGEventTapLocation::HID);

    std::thread::sleep(Duration::from_millis(50));

    let up = CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, point, CGMouseButton::Left)
        .map_err(|_| AppShotsError::InteractionFailed {
            action: "tap".into(),
            detail: "failed to create mouse up event".into(),
        })?;
    up.post(core_graphics::event::CGEventTapLocation::HID);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_action_returns_interaction_failed() {
        let result = handle_interact_simulator("swipe", None, None, None, None, 0).await;
        let err = result.unwrap_err();
        assert!(matches!(err, AppShotsError::InteractionFailed { .. }));
        assert!(err.to_string().contains("unknown action: swipe"));
    }

    #[tokio::test]
    async fn tap_without_x_returns_interaction_failed() {
        let result = handle_interact_simulator("tap", None, Some(100.0), None, None, 0).await;
        let err = result.unwrap_err();
        assert!(matches!(err, AppShotsError::InteractionFailed { .. }));
        assert!(err.to_string().contains("x coordinate is required"));
    }

    #[tokio::test]
    async fn tap_without_y_returns_interaction_failed() {
        let result = handle_interact_simulator("tap", Some(100.0), None, None, None, 0).await;
        let err = result.unwrap_err();
        assert!(matches!(err, AppShotsError::InteractionFailed { .. }));
        assert!(err.to_string().contains("y coordinate is required"));
    }

    #[test]
    fn interact_result_serializes() {
        let result = InteractResult {
            action: "scroll".into(),
            success: true,
        };
        let json = serde_json::to_value(&result).expect("should serialize");
        assert_eq!(json["action"], "scroll");
        assert_eq!(json["success"], true);
    }

    #[test]
    fn interact_result_tap_serializes() {
        let result = InteractResult {
            action: "tap".into(),
            success: true,
        };
        let json = serde_json::to_value(&result).expect("should serialize");
        assert_eq!(json["action"], "tap");
    }

    #[tokio::test]
    async fn empty_action_returns_interaction_failed() {
        let result = handle_interact_simulator("", None, None, None, None, 0).await;
        let err = result.unwrap_err();
        assert!(matches!(err, AppShotsError::InteractionFailed { .. }));
        assert!(err.to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn scroll_without_dy_uses_zero_default() {
        // Scroll with no dx/dy should attempt CGEvent (will fail without Accessibility)
        // but the error proves we got past parameter validation
        let result = handle_interact_simulator("scroll", None, None, None, None, 0).await;
        // Either succeeds (has Accessibility) or fails at CGEvent creation
        if let Err(e) = result {
            assert!(matches!(e, AppShotsError::InteractionFailed { .. }));
            // Should mention CGEventSource, not parameter validation
            assert!(
                e.to_string().contains("CGEventSource") || e.to_string().contains("event"),
                "expected CGEvent error, got: {e}"
            );
        }
    }

    #[tokio::test]
    async fn tap_with_both_coords_attempts_cgevent() {
        // With both x and y, should pass validation and attempt CGEvent
        let result =
            handle_interact_simulator("tap", Some(100.0), Some(200.0), None, None, 0).await;
        if let Err(e) = result {
            assert!(matches!(e, AppShotsError::InteractionFailed { .. }));
            // Should fail at CGEvent, not at parameter validation
            assert!(
                e.to_string().contains("CGEventSource") || e.to_string().contains("event"),
                "expected CGEvent error, got: {e}"
            );
        }
    }

    #[tokio::test]
    async fn scroll_with_custom_start_position_attempts_cgevent() {
        let result = handle_interact_simulator(
            "scroll",
            Some(500.0),
            Some(600.0),
            Some(0.0),
            Some(300.0),
            0,
        )
        .await;
        if let Err(e) = result {
            assert!(matches!(e, AppShotsError::InteractionFailed { .. }));
        }
    }
}
