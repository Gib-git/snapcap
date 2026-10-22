// Region selection logic, independent of any UI toolkit. The overlay feeds it pointer
// and key events in image-pixel coordinates of the display under the pointer, and draws
// whatever state it reports.
//
// Screenshot: drag out an area (released = captured), or click for the whole display.
// Record: drag out an area, then move it, resize it by its edges and corners, and confirm
// with Enter, a double-click or the toolbar. A click selects the whole display.
#pragma once

#include <cstddef>
#include <optional>
#include <vector>

#include "snapcap/geometry.h"
#include "snapcap/host.h"

namespace snapcap {

struct Edges {
  bool left = false, right = false, top = false, bottom = false;
  bool operator==(const Edges&) const = default;
  bool any() const { return left || right || top || bottom; }
};

// Which edges a pointer at `p` would resize, if it is on or near the border of `r`.
std::optional<Edges> handle_at(const RectF& r, PointF p, double tolerance);

class SelectionModel {
 public:
  struct Display {
    uint32_t width = 0;   // image pixels
    uint32_t height = 0;
    double px_per_point = 1.0;
  };
  enum class DragKind { None, Create, Move, Resize };
  // What the overlay should do after an event.
  struct Outcome {
    bool cancel = false;
    std::optional<Selection> confirm;
  };

  // Minimum selection size in image pixels; anything smaller counts as a click.
  static constexpr double kMinPx = 8.0;
  // Grab distance around edges and corners, in points.
  static constexpr double kHandlePt = 8.0;

  SelectionModel(SelectionPurpose purpose, std::vector<Display> displays);

  SelectionPurpose purpose() const { return purpose_; }
  // The current selection: (display index, rectangle in that display's image pixels).
  std::optional<std::pair<size_t, RectF>> selection() const { return sel_; }
  DragKind dragging() const { return drag_.kind; }
  // What a press at `p` would do (for choosing the cursor).
  DragKind hit(size_t display, PointF p, Edges* edges = nullptr) const;

  void press(size_t display, PointF p);
  void drag(size_t display, PointF p);
  Outcome release(size_t display, PointF p);
  Outcome double_click(size_t display);
  Outcome key_enter(size_t display);
  Outcome key_escape();
  // Toolbar: "Whole screen".
  void select_full(size_t display);
  // Toolbar: "Start recording".
  Outcome confirm_current();

 private:
  struct Drag {
    DragKind kind = DragKind::None;
    size_t display = 0;
    PointF origin;     // press position
    PointF grab;       // Move: pointer offset from the selection's top-left
    Edges edges;       // Resize: the edges being dragged
    RectF anchor;      // Resize: the selection when the drag began
    bool moved = false;
  };

  RectF full(size_t display) const;
  PointF clamp(size_t display, PointF p) const;
  Selection make(size_t display, RectF r) const;

  SelectionPurpose purpose_;
  std::vector<Display> displays_;
  std::optional<std::pair<size_t, RectF>> sel_;
  Drag drag_;
};

}  // namespace snapcap
