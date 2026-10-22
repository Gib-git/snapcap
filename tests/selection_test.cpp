#include <doctest/doctest.h>

#include "snapcap/selection.h"

using namespace snapcap;
using Kind = SelectionModel::DragKind;

namespace {
// Two displays: a 2x Retina one (2000x1000 px) and a 1x one.
SelectionModel model(SelectionPurpose p) {
  return SelectionModel(p, {{2000, 1000, 2.0}, {800, 600, 1.0}});
}
}  // namespace

TEST_CASE("handles detect corners and edges") {
  RectF r{100, 100, 200, 100};
  CHECK(handle_at(r, {101, 99}, 8) == Edges{true, false, true, false});
  CHECK(handle_at(r, {200, 201}, 8) == Edges{false, false, false, true});
  CHECK(handle_at(r, {299, 150}, 8) == Edges{false, true, false, false});
  CHECK_FALSE(handle_at(r, {200, 150}, 8).has_value());
  CHECK_FALSE(handle_at(r, {10, 10}, 8).has_value());
}

TEST_CASE("screenshot: dragging out an area captures it on release") {
  auto m = model(SelectionPurpose::Screenshot);
  m.press(0, {100, 50});
  m.drag(0, {400, 250});
  CHECK(m.dragging() == Kind::Create);
  m.drag(0, {40.4, 20.6});  // dragging up-left of the origin works too
  auto out = m.release(0, {40.4, 20.6});
  REQUIRE(out.confirm);
  CHECK(out.confirm->shot == 0);
  CHECK(out.confirm->rect == PxRect{40, 21, 60, 29});
  CHECK_FALSE(out.confirm->full);
}

TEST_CASE("screenshot: a click captures the whole display") {
  auto m = model(SelectionPurpose::Screenshot);
  m.press(1, {300, 300});
  m.drag(1, {301, 300});  // a tiny jitter is still a click
  auto out = m.release(1, {301, 300});
  REQUIRE(out.confirm);
  CHECK(out.confirm->shot == 1);
  CHECK(out.confirm->full);
  CHECK(out.confirm->rect == PxRect{0, 0, 800, 600});
}

TEST_CASE("screenshot: a sliver smaller than the minimum is discarded") {
  auto m = model(SelectionPurpose::Screenshot);
  m.press(0, {100, 100});
  m.drag(0, {300, 104});
  auto out = m.release(0, {300, 104});
  CHECK_FALSE(out.confirm);
  CHECK_FALSE(m.selection());
}

TEST_CASE("record: create, move, resize, then confirm") {
  auto m = model(SelectionPurpose::Record);
  m.press(0, {100, 100});
  m.drag(0, {500, 400});
  CHECK_FALSE(m.release(0, {500, 400}).confirm);  // recording waits for a confirmation
  REQUIRE(m.selection());
  CHECK(m.selection()->second == RectF{100, 100, 400, 300});

  // Inside: move, clamped to the display.
  CHECK(m.hit(0, {300, 250}) == Kind::Move);
  m.press(0, {300, 250});
  m.drag(0, {5000, 250});
  m.release(0, {5000, 250});
  CHECK(m.selection()->second == RectF{1600, 100, 400, 300});

  // The bottom-right corner (within 8 pt = 16 px): resize.
  Edges e;
  CHECK(m.hit(0, {2010, 395}, &e) == Kind::Resize);
  CHECK(e == Edges{false, true, false, true});
  m.press(0, {1990, 395});
  m.drag(0, {1700, 300});
  m.release(0, {1700, 300});
  CHECK(m.selection()->second == RectF{1600, 100, 100, 200});

  // Dragging the left edge past the right edge stops at the minimum size.
  m.press(0, {1600, 200});
  m.drag(0, {1900, 200});
  m.release(0, {1900, 200});
  CHECK(m.selection()->second.width == SelectionModel::kMinPx);

  auto out = m.key_enter(0);
  REQUIRE(out.confirm);
  CHECK(out.confirm->rect == PxRect{1692, 100, 8, 200});
}

TEST_CASE("record: a click selects the whole display; double-click confirms") {
  auto m = model(SelectionPurpose::Record);
  m.press(1, {10, 10});
  CHECK_FALSE(m.release(1, {10, 10}).confirm);
  REQUIRE(m.selection());
  CHECK(m.selection()->first == 1);
  auto out = m.double_click(1);
  REQUIRE(out.confirm);
  CHECK(out.confirm->full);
}

TEST_CASE("record: a new drag on another display replaces the selection") {
  auto m = model(SelectionPurpose::Record);
  m.select_full(0);
  CHECK(m.hit(1, {50, 50}) == Kind::Create);
  m.press(1, {50, 50});
  m.drag(1, {150, 150});
  m.release(1, {150, 150});
  CHECK(m.selection()->first == 1);
  auto out = m.confirm_current();
  REQUIRE(out.confirm);
  CHECK(out.confirm->shot == 1);
}

TEST_CASE("keys: Enter without a selection takes the display; Escape cancels") {
  auto m = model(SelectionPurpose::Screenshot);
  auto out = m.key_enter(1);
  REQUIRE(out.confirm);
  CHECK(out.confirm->full);
  CHECK(m.key_escape().cancel);
}

TEST_CASE("events for a display other than the dragged one are ignored") {
  auto m = model(SelectionPurpose::Screenshot);
  m.press(0, {100, 100});
  m.drag(1, {500, 500});
  CHECK_FALSE(m.selection());
  CHECK(m.release(1, {500, 500}).confirm.has_value());  // the press on display 0 was a click
}
