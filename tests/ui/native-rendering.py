#!/usr/bin/env python3
"""Native rendering regression; samples GDK presentation, not DOM opacity.

Build fixtures: npm run test:ui -- --list
Run under the same environment as the debug app:
  python tests/ui/native-rendering.py --compositor compatible
Compare system WebKit defaults with --compositor system (known red on 2.54.1).
Use --wheel-input x11 to exercise mouse-wheel input through X11/XTest rather
than injecting scroll events directly into GTK (requires libX11 and libXtst).
The temporary test window must remain visible and unobscured while it runs.
"""
import gi, json, mimetypes, sys
from collections import defaultdict
from pathlib import Path
from urllib.parse import urlparse, unquote
gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
gi.require_version('Soup', '3.0')
from gi.repository import Gtk, WebKit2, Gio, GLib, Soup, Gdk

import argparse
import ctypes
parser = argparse.ArgumentParser(description='Capture actual WebKitGTK window pixels during artwork and dialog fades. Requires GTK3, WebKit2 4.1, PyGObject, a visible X11 display, and npm run test:ui -- --list.')
parser.add_argument('--compositor', choices=['system', 'compatible'], default='compatible')
parser.add_argument('--wheel-input', choices=['gdk', 'x11'], default='gdk')
args = parser.parse_args()
mode = args.compositor
root = Path(__file__).resolve().parents[2] / '.test-dist'
context = WebKit2.WebContext.new_ephemeral()
def serve(request):
    path = root / unquote(urlparse(request.get_uri()).path).lstrip('/')
    data = path.read_bytes()
    response = WebKit2.URISchemeResponse.new(Gio.MemoryInputStream.new_from_data(data, None), len(data))
    response.set_content_type(mimetypes.guess_type(path)[0] or 'application/octet-stream')
    headers = Soup.MessageHeaders.new(Soup.MessageHeadersType.RESPONSE)
    headers.append('Access-Control-Allow-Origin', '*')
    response.set_http_headers(headers)
    request.finish_with_response(response)
context.register_uri_scheme('art-test', serve)
security = context.get_security_manager()
security.register_uri_scheme_as_secure('art-test')
security.register_uri_scheme_as_local('art-test')
security.register_uri_scheme_as_cors_enabled('art-test')
view = WebKit2.WebView(web_context=context)
settings = view.get_settings()
features = settings.get_all_features()
for i in range(features.get_length()):
    feature = features.get(i)
    name = feature.get_identifier()
    if name in ['UseSkiaForComposition', 'UseDamagingInformationForCompositing', 'PropagateDamagingInformation']:
        if mode == 'compatible': settings.set_feature_enabled(feature, False)
        print(name, settings.get_feature_enabled(feature), flush=True)
window = Gtk.Window(title='TagRex native flash check')
window.set_default_size(1100, 900)
window.set_decorated(False)
window.add(view)
window.show_all()
samples = []
wheel_results = []
exit_code = 2
def message(manager, result):
    value = json.loads(result.get_js_value().to_string())
    if value.get('done'):
        groups = defaultdict(list)
        for item in samples: groups[item['phase']].append(item)
        failures = []
        for phase, frames in groups.items():
            sign = 1 if phase.startswith('close') or (phase.startswith('switch') and int(phase.split('-')[1]) % 2) else -1
            channel = 0 if phase.startswith('switch') else 2
            for a, b in zip(frames, frames[1:]):
                if (b['rgb'][channel] - a['rgb'][channel]) * sign < -20:
                    failures.append({'phase': phase, 'at_ms': round(b['t']), 'from': a['rgb'], 'to': b['rgb']})
        global exit_code
        calibrated = len(groups) == 24 and all(len(frames) > 10 for frames in groups.values()) and all(frames[-1]['rgb'] in ([182,69,50], [50,79,182]) for phase,frames in groups.items() if phase.startswith('switch'))
        colour_errors = sum(abs(item['rgb'][0] + item['rgb'][2] - 232) > 5 for item in samples if item['phase'].startswith('switch'))
        wheel_ok = len(wheel_results) == 2 and all(result['after'] > result['before'] * 1.05 for result in wheel_results)
        exit_code = int(bool(failures) or not calibrated or colour_errors or not wheel_ok)
        print(json.dumps({'mode':mode,'samples':len(samples),'phases':len(groups),'calibrated':calibrated,'flashes':len(failures),'colour_errors':colour_errors,'wheel_zoom':wheel_results,'examples':failures[:3]}), flush=True)
        Gtk.main_quit()
        return
    if 'error' in value:
        print(value, flush=True); Gtk.main_quit(); return
    if 'scroll' in value:
        coords = value['scroll']
        for i in range(3):
            if args.wheel_input == 'x11':
                def physical_scroll():
                    xlib = ctypes.CDLL('libX11.so.6')
                    xtest = ctypes.CDLL('libXtst.so.6')
                    xlib.XOpenDisplay.restype = ctypes.c_void_p
                    display = xlib.XOpenDisplay(None)
                    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
                    xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
                    xlib.XFlush.argtypes = [ctypes.c_void_p]
                    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
                    origin = view.get_window().get_origin()
                    xtest.XTestFakeMotionEvent(display, -1, int(origin[-2] + coords['x']), int(origin[-1] + coords['y']), 0)
                    xtest.XTestFakeButtonEvent(display, 4, 1, 0)
                    xtest.XTestFakeButtonEvent(display, 4, 0, 0)
                    xlib.XFlush(display)
                    xlib.XCloseDisplay(display)
                    return False
                GLib.timeout_add(150 + i * 200, physical_scroll)
                continue
            event = Gdk.Event.new(Gdk.EventType.SCROLL)
            event.scroll.window = view.get_window()
            event.scroll.send_event = True
            event.scroll.time = Gdk.CURRENT_TIME
            event.scroll.x = coords['x']
            event.scroll.y = coords['y']
            event.scroll.direction = Gdk.ScrollDirection.UP
            event.set_device(view.get_display().get_default_seat().get_pointer())
            GLib.timeout_add(150 + i * 200, lambda e=event: (Gtk.main_do_event(e), False)[1])
        return
    if 'wheel_result' in value:
        wheel_results.append(value['wheel_result'])
        return
    pixel = Gdk.pixbuf_get_from_window(view.get_window(), int(value['x']), int(value['y']), 1, 1)
    if pixel:
        value['rgb'] = list(pixel.get_pixels()[:3])
        samples.append(value)
manager = view.get_user_content_manager()
manager.register_script_message_handler('probe')
manager.connect('script-message-received::probe', message)
script = r'''
(async () => {
 const send = x => window.webkit.messageHandlers.probe.postMessage(JSON.stringify(x));
 const wait = ms => new Promise(r => setTimeout(r, ms));
 const until = async f => { for(let i=0;i<300;i++){if(f())return;await wait(20);} throw Error('timeout'); };
 await until(() => window.artworkTest);
 let phase = '', start = 0, x = 0, y = 0, recording = true;
 function sample(t) { if(!recording)return; if(phase)send({phase,t:t-start,x,y}); requestAnimationFrame(sample); }
 requestAnimationFrame(sample);
 const launch = () => document.querySelector('#launch').click();
 const thumb = i => document.querySelector(`.aw-thumb[data-index="${i}"]`).click();
 launch(); await until(() => document.querySelector('.aw-use:not(:disabled)')); await wait(450);
 const bounds = document.querySelector('.aw-stage').getBoundingClientRect();
 x = bounds.x + bounds.width / 2; y = bounds.y + bounds.height / 2;
 for(let i=0;i<12;i++) { phase = `switch-${i}`;start=performance.now();thumb((i+1)%2);await wait(420); }
 phase=''; document.querySelector('.aw-close').click();await wait(400);
 for(let i=0;i<6;i++) {
  phase=`open-${i}`;start=performance.now();launch();await wait(550);
  phase=`close-${i}`;start=performance.now();document.querySelector('.aw-close').click();await wait(450);
 }
 phase='';
 launch();await until(()=>document.querySelector('.aw-use:not(:disabled)'));await wait(400);
 document.querySelector('.aw-surface').click();await wait(250);
 for(const selector of ['.aw-surface', '.aw-scale']) {
  const b=document.querySelector(selector).getBoundingClientRect();
  const before=Number(document.querySelector('.aw-viewer').dataset.scale);
  send({scroll:{x:b.x+b.width/2,y:b.y+b.height/2}});await wait(1100);
  send({wheel_result:{target:selector,before,after:Number(document.querySelector('.aw-viewer').dataset.scale)}});
 }
 recording=false;send({done:true});
})().catch(e=>window.webkit.messageHandlers.probe.postMessage(JSON.stringify({error:String(e)})))
'''
def loaded(v, event):
    if event == WebKit2.LoadEvent.FINISHED: v.evaluate_javascript(script, -1, None, None, None, None, None)
view.connect('load-changed', loaded)
view.load_uri('art-test://fixture/tests/ui/fixture/index.html?count=3')
GLib.timeout_add_seconds(25, lambda: (Gtk.main_quit(), False)[1])
Gtk.main()
window.destroy()
sys.exit(exit_code)
