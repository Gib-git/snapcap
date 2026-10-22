#include "snapcap/selection.h"

#include <algorithm>
#include <cmath>

namespace snapcap {

std::optional<Edges> handle_at(const RectF& r, PointF p, double tol) {
  RectF grown{r.x - tol, r.y - tol, r.width + 2 * tol, r.height + 2 * tol};
  if (p.x < grown.x || p.y < grown.y || p.x > grown.right() || p.y > grown.bottom()) return std::nullopt;
  Edges e;
  e.left = std::abs(p.x - r.x) <= tol;
  e.right = !e.left && std::abs(p.x - r.right()) <= tol;
  e.top = std::abs(p.y - r.y) <= tol;
  e.bottom = !e.top && std::abs(p.y - r.bottom()) <= tol;
  if (!e.any()) return std::nullopt;
  return e;
}

SelectionModel::SelectionModel(SelectionPurpose purpose, std::vector<Display> displays)
    : purpose_(purpose), displays_(std::move(displays)) {}

RectF SelectionModel::full(size_t d) const {
  return {0, 0, double(displays_[d].width), double(displays_[d].height)};
}

PointF SelectionModel::clamp(size_t d, PointF p) const {
  return {std::clamp(p.x, 0.0, double(displays_[d].width)), std::clamp(p.y, 0.0, double(displays_[d].height))};
}

Selection SelectionModel::make(size_t d, RectF r) const {
  RectF f = full(d);
  double x0 = std::clamp(r.x, 0.0, f.width), y0 = std::clamp(r.y, 0.0, f.height);
  double x1 = std::clamp(r.right(), 0.0, f.width), y1 = std::clamp(r.bottom(), 0.0, f.height);
  bool is_full = std::abs((x1 - x0) - f.width) < 1.0 && std::abs((y1 - y0) - f.height) < 1.0;
  auto px = [](double v) { return static_cast<uint32_t>(std::lround(v)); };
  return Selection{d, PxRect{px(x0), px(y0), std::max(1u, px(x1 - x0)), std::max(1u, px(y1 - y0))}, is_full};
}

SelectionModel::DragKind SelectionModel::hit(size_t d, PointF p, Edges* edges) const {
  if (purpose_ != SelectionPurpose::Record || !sel_ || sel_->first != d) return DragKind::Create;
  const RectF& r = sel_->second;
  double tol = kHandlePt * displays_[d].px_per_point;
  if (auto e = handle_at(r, p, tol)) {
    if (edges != nullptr) *edges = *e;
    return DragKind::Resize;
  }
  return r.contains(p) ? DragKind::Move : DragKind::Create;
}

void SelectionModel::press(size_t d, PointF p) {
  if (d >= displays_.size()) return;
  p = clamp(d, p);
  Drag next;
  next.display = d;
  next.origin = p;
  next.kind = hit(d, p, &next.edges);
  if (next.kind == DragKind::Move) next.grab = {p.x - sel_->second.x, p.y - sel_->second.y};
  if (next.kind == DragKind::Resize) next.anchor = sel_->second;
  drag_ = next;
}

void SelectionModel::drag(size_t d, PointF p) {
  if (drag_.kind == DragKind::None || d != drag_.display) return;
  p = clamp(d, p);
  if (!drag_.moved && std::hypot(p.x - drag_.origin.x, p.y - drag_.origin.y) < 2.0 * displays_[d].px_per_point) return;
  drag_.moved = true;
  switch (drag_.kind) {
    case DragKind::Create: {
      double x0 = std::min(drag_.origin.x, p.x), y0 = std::min(drag_.origin.y, p.y);
      sel_ = {{d, RectF{x0, y0, std::abs(p.x - drag_.origin.x), std::abs(p.y - drag_.origin.y)}}};
      break;
    }
    case DragKind::Move: {
      RectF r = sel_->second;
      RectF f = full(d);
      r.x = std::clamp(p.x - drag_.grab.x, 0.0, f.width - r.width);
      r.y = std::clamp(p.y - drag_.grab.y, 0.0, f.height - r.height);
      sel_->second = r;
      break;
    }
    case DragKind::Resize: {
      const RectF& a = drag_.anchor;
      double x0 = a.x, y0 = a.y, x1 = a.right(), y1 = a.bottom();
      if (drag_.edges.left) x0 = std::min(p.x, x1 - kMinPx);
      if (drag_.edges.right) x1 = std::max(p.x, x0 + kMinPx);
      if (drag_.edges.top) y0 = std::min(p.y, y1 - kMinPx);
      if (drag_.edges.bottom) y1 = std::max(p.y, y0 + kMinPx);
      sel_->second = RectF{x0, y0, x1 - x0, y1 - y0};
      break;
    }
    case DragKind::None: break;
  }
}

SelectionModel::Outcome SelectionModel::release(size_t d, PointF p) {
  drag(d, p);  // the release position is the final one
  Drag done = drag_;
  drag_ = Drag{};
  if (done.kind == DragKind::None) return {};
  if (!done.moved) {
    // A click.
    if (purpose_ == SelectionPurpose::Screenshot) return {false, make(done.display, full(done.display))};
    if (!sel_ || sel_->first != done.display) sel_ = {{done.display, full(done.display)}};
    return {};
  }
  if (sel_ && (sel_->second.width < kMinPx || sel_->second.height < kMinPx)) {
    sel_.reset();
    return {};
  }
  if (purpose_ == SelectionPurpose::Screenshot && sel_) return {false, make(sel_->first, sel_->second)};
  return {};
}

SelectionModel::Outcome SelectionModel::double_click(size_t) {
  if (purpose_ == SelectionPurpose::Record && sel_) return {false, make(sel_->first, sel_->second)};
  return {};
}

SelectionModel::Outcome SelectionModel::key_enter(size_t d) {
  if (sel_) return {false, make(sel_->first, sel_->second)};
  if (d < displays_.size()) return {false, make(d, full(d))};
  return {};
}

SelectionModel::Outcome SelectionModel::key_escape() {
  return {true, std::nullopt};
}

void SelectionModel::select_full(size_t d) {
  if (d < displays_.size()) sel_ = {{d, full(d)}};
}

SelectionModel::Outcome SelectionModel::confirm_current() {
  if (sel_) return {false, make(sel_->first, sel_->second)};
  return {};
}

}  // namespace snapcap
