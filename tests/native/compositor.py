#!/usr/bin/env python3
"""Sample presented GTK window pixels during a standalone CSS opacity fade."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import sys
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, GLib, Gtk, WebKit2

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--compositor", choices=["system", "compatible"], required=True)
args = parser.parse_args()
view = WebKit2.WebView(web_context=WebKit2.WebContext.new_ephemeral())
settings = view.get_settings()
features = settings.get_all_features()
for index in range(features.get_length()):
    feature = features.get(index)
    if feature.get_identifier() in {
        "UseSkiaForComposition", "UseDamagingInformationForCompositing",
        "PropagateDamagingInformation",
    }:
        if args.compositor == "compatible":
            settings.set_feature_enabled(feature, False)
        print(feature.get_identifier(), settings.get_feature_enabled(feature), flush=True)
window = Gtk.Window(title="WebKitGTK fade probe — keep unobscured")
window.set_default_size(400, 300)
window.add(view)
samples = defaultdict(list)
exit_code = 2

def receive(_manager, result):
    global exit_code
    value = json.loads(result.get_js_value().to_string())
    if "error" in value:
        print(value, flush=True)
        Gtk.main_quit()
    elif value.get("done"):
        reversals = []
        for phase, frames in samples.items():
            direction = 1 if phase.startswith("open") else -1
            for before, after in zip(frames, frames[1:]):
                if (after["red"] - before["red"]) * direction < -20:
                    reversals.append({"phase": phase, "before": before, "after": after})
        calibrated = len(samples) == 12 and all(
            len(frames) >= 10 and abs(frames[-1]["red"] - (182 if phase.startswith("open") else 50)) <= 3
            for phase, frames in samples.items()
        )
        exit_code = int(not calibrated or bool(reversals))
        print(json.dumps({"mode": args.compositor, "calibrated": calibrated,
                          "phases": len(samples), "reversals": reversals}), flush=True)
        Gtk.main_quit()
    else:
        pixels = Gdk.pixbuf_get_from_window(view.get_window(), value["x"], value["y"], 1, 1)
        if pixels:
            samples[value["phase"]].append({"t": value["t"], "red": pixels.get_pixels()[0]})

manager = view.get_user_content_manager()
manager.register_script_message_handler("probe")
manager.connect("script-message-received::probe", receive)
window.show_all()
view.load_uri(Path(__file__).with_suffix(".html").resolve().as_uri())
GLib.timeout_add_seconds(15, lambda: (Gtk.main_quit(), False)[1])
Gtk.main()
window.destroy()
sys.exit(exit_code)
