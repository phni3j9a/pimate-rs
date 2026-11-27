/* Xvfb-only text owner: refuses TARGETS, supports UTF8_STRING. */
#include <X11/Xlib.h>
#include <stdio.h>

int main(void) {
    Display *d = XOpenDisplay(NULL);
    if (!d) return 1;
    Window w = XCreateSimpleWindow(d, DefaultRootWindow(d), 0, 0, 1, 1, 0, 0, 0);
    Atom clipboard = XInternAtom(d, "CLIPBOARD", False);
    Atom targets = XInternAtom(d, "TARGETS", False);
    Atom utf8 = XInternAtom(d, "UTF8_STRING", False);
    XSetSelectionOwner(d, clipboard, w, CurrentTime);
    if (XGetSelectionOwner(d, clipboard) != w) return 1;
    puts("ready"); fflush(stdout);
    int queries = 0;
    for (;;) {
        XEvent event;
        XNextEvent(d, &event);
        if (event.type != SelectionRequest) continue;
        XSelectionRequestEvent r = event.xselectionrequest;
        XEvent reply = {.xselection = {.type = SelectionNotify, .display = d, .requestor = r.requestor,
            .selection = r.selection, .target = r.target, .time = r.time, .property = None}};
        if (r.target == utf8) {
            const char text[] = "PiMate clipboard text";
            XChangeProperty(d, r.requestor, r.property, utf8, 8, PropModeReplace,
                            (const unsigned char *)text, sizeof(text) - 1);
            reply.xselection.property = r.property;
        } else if (r.target == targets && ++queries > 1) {
            /* Only the original GPUI paste should query TARGETS, not an image probe. */
            fprintf(stderr, "unexpected extra TARGETS request\n");
            return 1;
        }
        XSendEvent(d, r.requestor, False, NoEventMask, &reply);
        XFlush(d);
    }
}
