use serde::Deserialize;

use crate::maths_utility::{self, Rect, Vec2, MinMax};
use crate::config::{Padding, Color};
use crate::rendering::window::NotifyWindow;
use crate::bus::dbus::Notification;
use crate::rendering::layout::{LayoutBlock, DrawableLayoutElement, Hook};
use crate::rendering::text::EllipsizeMode;
use std::time::Duration;


#[derive(Debug, Deserialize, Clone)]
pub struct ScrollingTextBlockParameters {
    pub padding: Padding,
    pub text: String,
    pub font: String,
    pub color: Color,

    pub width: MinMax,

    pub scroll_speed: f64,
    pub lhs_dist: f64,
    pub rhs_dist: f64,
    pub scroll_t: f64,

    // Optional fields ----
    pub width_image_hint: Option<MinMax>,
    pub width_image_app: Option<MinMax>,
    pub width_image_both: Option<MinMax>,
    #[serde(default)]
    pub render_when_empty: bool,

    #[serde(skip)]
    real_text: String,

    #[serde(skip)]
    clip_rect: Rect,
    #[serde(skip)]
    text_rect: Rect,
    #[serde(skip)]
    scroll_distance: f64,
    #[serde(skip)]
    real_width: MinMax,

    #[serde(skip)]
    update_enabled: bool,
}

impl ScrollingTextBlockParameters {
    fn get_width(&self, notification: &Notification) -> &MinMax {
        match (notification.app_image.is_some(), notification.hint_image.is_some()) {
            (true, true) => self.width_image_both.as_ref().unwrap_or(&self.width),
            (true, false) => self.width_image_app.as_ref().unwrap_or(&self.width),
            (false, true) => self.width_image_hint.as_ref().unwrap_or(&self.width),
            (false, false) => &self.width,
        }
    }
}

impl DrawableLayoutElement for ScrollingTextBlockParameters {
    fn draw(&self, hook: &Hook, offset: &Vec2, parent_rect: &Rect, window: &NotifyWindow) -> Rect {
        // Sometimes users might want to render empty blocks to maintain padding and stuff, so we
        // optionally allow it.
        if self.real_text.is_empty() && !self.render_when_empty {
            let pos = LayoutBlock::find_anchor_pos(hook, offset, parent_rect, &Rect::EMPTY);
            return Rect::new(pos.x, pos.y, 0.0, 0.0);
        }

        let width = &self.real_width;

        // First, generate bounding rect with padding and stuff -- the space the text will
        // physically occupy.
        // We could cache this rect, but haven't yet.
        // We need to set some ellipsize mode, or the text size will be forced larger despite our
        // max width/height.
        window.text.set_text(&self.real_text, &self.font, width.max, 0, &EllipsizeMode::Middle);
        let mut rect = window.text.get_sized_padded_rect(&self.padding, width.min, 0);

        // Set the text to the real (scrolling) string.
        window.text.set_text(&self.real_text, &self.font, -1, 0, &EllipsizeMode::NoEllipsize);

        let mut pos = LayoutBlock::find_anchor_pos(hook, offset, parent_rect, &rect);
        pos.x += self.padding.left;
        pos.y += self.padding.top;
        // Debug, unpadded drawing, to help users.
        maths_utility::debug_rect(&window.context, true, pos.x, pos.y, self.clip_rect.width(), self.clip_rect.height());

        // If we're larger than the max size, then we should scroll, which is just changing the
        // text's x position really.
        if self.text_rect.width() > width.max as f64 {
            window.context.rectangle(
                pos.x,
                pos.y,
                self.clip_rect.width(),
                self.clip_rect.height()
            );
            window.context.clip();

            // @TODO: also add dynamic scroll option.
            // Equivalent to clip_rect.left() + self.lhs_dist if clip_rect had correct coordinates.
            let bounce_left = pos.x + self.padding.left + self.lhs_dist;
            // Equivalent to clip_rect.right() - self.rhs_dist - text_rect.width() if clip_rect had
            // correct coordinates.
            let bounce_right =
                pos.x + self.padding.left + self.clip_rect.width() - self.rhs_dist - self.text_rect.width();

            let lerp = maths_utility::lerp(bounce_right, bounce_left, self.scroll_t);
            // Keep track of pos.x; it's important for the layout.
            let temp = pos.x;
            pos.x = lerp;
            window.text.paint(&window.context, &pos, &self.color);
            pos.x = temp;
        } else {
            window.text.paint(&window.context, &pos, &self.color);
        }

        pos.x -= self.padding.left;
        pos.y -= self.padding.top;

        rect.set_xy(pos.x, pos.y,);
        rect
    }

    fn predict_rect_and_init(&mut self, hook: &Hook, offset: &Vec2, parent_rect: &Rect, window: &NotifyWindow) -> Rect {
        let text = maths_utility::format_notification_string(&self.text, &window.notification);

        if text.is_empty() && !self.render_when_empty {
            self.update_enabled = false;
            self.real_text = text;
            let pos = LayoutBlock::find_anchor_pos(hook, offset, parent_rect, &Rect::EMPTY);
            return Rect::new(pos.x, pos.y, 0.0, 0.0);
        }

        // We cache real_width because we need to access it in `update()` later, which doesn't have
        // access to the notification.
        self.real_width = self.get_width(&window.notification).clone();

        // Max height of 0 = one line of text.
        window.text.set_text(&text, &self.font, self.real_width.max, 0, &EllipsizeMode::Middle);

        // `rect`      -- Padded rect, for calculating bounding box.
        // `clip_rect` -- Unpadded rect, used for clipping.
        // `text_rect` -- Real text rect, with infinite length.
        let mut rect = window.text.get_sized_padded_rect(&self.padding, self.real_width.min, 0);
        let clip_rect = window.text.get_sized_padded_rect(&Padding::new(0.0, 0.0, 0.0, 0.0), 0, 0);
        window.text.set_text(&text, &self.font, -1, 0, &EllipsizeMode::NoEllipsize);
        let text_rect = window.text.get_sized_padded_rect(&self.padding, 0, 0);

        if text_rect.width() > self.real_width.max as f64 {
            self.update_enabled = true;
        }

        let pos = LayoutBlock::find_anchor_pos(hook, offset, parent_rect, &rect);

        // @TODO: also add dynamic scroll option.
        // `bounce_left`  -- Equivalent to clip_rect.left() + self.lhs_dist if clip_rect had correct coordinates.
        // `bounce_right` -- Equivalent to clip_rect.right() - self.rhs_dist - text_rect.width() if clip_rect had
        // correct coordinates.
        let bounce_left = pos.x + self.padding.left + self.lhs_dist;
        let bounce_right = pos.x + self.padding.left + clip_rect.width() - self.rhs_dist - text_rect.width();

        self.real_text = text;
        self.text_rect = text_rect;
        self.clip_rect = clip_rect;
        self.scroll_distance = maths_utility::distance(bounce_left, bounce_right);

        rect.set_xy(pos.x, pos.y);
        rect
    }

    fn update(&mut self, delta_time: Duration, _window: &NotifyWindow) -> bool {
        if !self.update_enabled {
            return false;
        }

        let width = &self.real_width;

        // Increase proportionally to distance (text width).
        self.scroll_t +=
            delta_time.as_secs_f64() * self.scroll_speed * (width.max as f64 / self.scroll_distance);

        // If scrolling right.
        if self.scroll_speed > 0.0 {
            // If reached right edge, reverse.
            if self.scroll_t >= 1.0 {
                self.scroll_speed = -self.scroll_speed;
            }
        } else if self.scroll_speed < 0.0 {
            // If reached left edge, reverse.
            if self.scroll_t <= 0.0 {
                self.scroll_speed = -self.scroll_speed;
            }
        }

        true
    }
}

