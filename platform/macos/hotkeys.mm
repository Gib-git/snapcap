#include "hotkeys.h"

#include <Carbon/Carbon.h>

#include <format>

#include "keymap.h"
#include "snapcap/log.h"

namespace snapcap::mac {

namespace {
constexpr OSType kSignature = 'SnCp';
}

struct GlobalHotkeys::Impl {
  Handler handler;
  EventHandlerRef event_handler = nullptr;
  std::map<uint32_t, std::pair<Action, EventHotKeyRef>> registered;
  uint32_t next_id = 1;

  static OSStatus on_event(EventHandlerCallRef, EventRef event, void* user) {
    EventHotKeyID hk{};
    if (GetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID, nullptr, sizeof(hk), nullptr, &hk) != noErr ||
        hk.signature != kSignature) {
      return eventNotHandledErr;
    }
    auto* self = static_cast<Impl*>(user);
    auto it = self->registered.find(hk.id);
    if (it == self->registered.end()) return eventNotHandledErr;
    Action action = it->second.first;
    // Carbon delivers on the main thread; defer so handlers never re-enter registration.
    Handler handler = self->handler;
    dispatch_async(dispatch_get_main_queue(), ^{
      handler(action);
    });
    return noErr;
  }
};

GlobalHotkeys::GlobalHotkeys(Handler handler) : impl_(new Impl) {
  impl_->handler = std::move(handler);
  EventTypeSpec spec{kEventClassKeyboard, kEventHotKeyPressed};
  OSStatus st = InstallApplicationEventHandler(&Impl::on_event, 1, &spec, impl_, &impl_->event_handler);
  if (st != noErr) log::error("cannot install the hotkey handler ({})", st);
}

GlobalHotkeys::~GlobalHotkeys() {
  clear();
  if (impl_->event_handler != nullptr) RemoveEventHandler(impl_->event_handler);
  delete impl_;
}

Result<void> GlobalHotkeys::add(Action action, const Shortcut& sc) {
  auto code = key_code(sc.key);
  if (!code) return fail(std::format("{} can't be used as a shortcut on macOS", sc.key));
  EventHotKeyID id{kSignature, impl_->next_id++};
  EventHotKeyRef ref = nullptr;
  OSStatus st = RegisterEventHotKey(*code, carbon_modifiers(sc.mods), id, GetApplicationEventTarget(), 0, &ref);
  if (st != noErr) return fail(st == eventHotKeyExistsErr ? "already registered" : std::format("error {}", st));
  impl_->registered[id.id] = {action, ref};
  return {};
}

void GlobalHotkeys::clear() {
  for (auto& [id, entry] : impl_->registered) UnregisterEventHotKey(entry.second);
  impl_->registered.clear();
}

}  // namespace snapcap::mac
