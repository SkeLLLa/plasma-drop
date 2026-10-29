"use strict";

let cmds = {};
let kwin = {};
let log = {};
let bridge = {};

log.log = (level, msg) => {
    bridge.log(level, msg);
};
log.error = (msg) => log.log("ERR", msg);
log.info = (msg) => log.log("INF", msg);
log.warning = (msg) => log.log("WRN", msg);

const mapWindow = (window) => ({
    internalId: window.internalId.toString(),
    desktopFileName: window.desktopFileName || "",
    resourceClass: window.resourceClass || "",
    resourceName: window.resourceName || "",
    caption: window.caption || "",
    frameGeometry: {
        x: Math.round(window.frameGeometry.x),
        y: Math.round(window.frameGeometry.y),
        width: Math.round(window.frameGeometry.width),
        height: Math.round(window.frameGeometry.height),
    },
    noBorder: Boolean(window.noBorder),
    minimized: Boolean(window.minimized),
    // KWin's MaximizeMode: 1 vertical, 2 horizontal, 3 both.
    maximizedVertically: Boolean(window.maximizeMode & 1),
    maximizedHorizontally: Boolean(window.maximizeMode & 2),
    fullScreen: Boolean(window.fullScreen),
});

kwin.getWindows = () => workspace.windowList();
kwin.getActiveWindow = () => workspace.activeWindow;
kwin.setActiveWindow = (window) => {
    workspace.activeWindow = window;
};
kwin.getWindowByInternalId = (internalId) => {
    for (const window of kwin.getWindows()) {
        if (window.internalId && window.internalId.toString() === internalId) {
            return window;
        }
    }
    return null;
};
kwin.getWindowByInternalIdRequired = (internalId) => {
    const window = kwin.getWindowByInternalId(internalId);
    if (!window) {
        throw new Error(`No window found with internal id ${internalId}`);
    }
    return window;
};

kwin.getScreenByNameRequired = (name) => {
    // Array.from also accepts the array-like list wrapper QJSEngine may hand out.
    const screen = Array.from(workspace.screens || []).find(
        (output) => String(output.name || "") === name
    );
    if (!screen) {
        throw new Error(`No screen named '${name}'`);
    }
    return screen;
};

bridge.DBUS_SERVICE = "ua.SkeLLLa.PlasmaDrop";
bridge.DBUS_PATH = "/ua/SkeLLLa/PlasmaDrop";
bridge.DBUS_INTERFACE = "ua.SkeLLLa.PlasmaDrop";

bridge.log = (level, msg) => {
    callDBus(
        bridge.DBUS_SERVICE,
        bridge.DBUS_PATH,
        bridge.DBUS_INTERFACE,
        "Log",
        level,
        msg
    );
};

bridge.getNextCommand = () => {
    callDBus(
        bridge.DBUS_SERVICE,
        bridge.DBUS_PATH,
        bridge.DBUS_INTERFACE,
        "GetNextCommand",
        bridge.onGotCommand
    );
};

bridge.onFocusChanged = (window) => {
    callDBus(
        bridge.DBUS_SERVICE,
        bridge.DBUS_PATH,
        bridge.DBUS_INTERFACE,
        "OnFocusChanged",
        window && window.internalId ? window.internalId.toString() : ""
    );
};

bridge.sendResponse = (cmdInfo, params, exceptionMessage) => {
    callDBus(
        bridge.DBUS_SERVICE,
        bridge.DBUS_PATH,
        bridge.DBUS_INTERFACE,
        "SendResponse",
        JSON.stringify({
            cmdType: cmdInfo.type,
            responderId: cmdInfo.responderId,
            params: params,
            exception_message: exceptionMessage || null,
        })
    );
};

const errorMessage = (error) => String(error && error.message ? error.message : error);

// Returned by a command that sends its response later itself.
bridge.DEFERRED = {};

bridge.onGotCommand = (cmdInfoStr) => {
    // Always re-arm the long poll, even when parsing or replying throws; otherwise the
    // bridge stops for good and the daemon only notices through command timeouts.
    let cmdInfo = null;
    try {
        cmdInfo = JSON.parse(cmdInfoStr);
        // The daemon answers an idle long poll with NOOP; it expects no response.
        if (cmdInfo.type === "NOOP") {
            return;
        }
        const cmd = cmds[cmdInfo.type];
        if (typeof cmd !== "function") {
            throw new Error(`Unknown command '${cmdInfo.type}'`);
        }
        const params = cmd(cmdInfo.params || {}, cmdInfo) || {};
        if (params !== bridge.DEFERRED) {
            bridge.sendResponse(cmdInfo, params, null);
        }
    } catch (error) {
        try {
            log.error(errorMessage(error));
            if (cmdInfo) {
                bridge.sendResponse(cmdInfo, {}, errorMessage(error));
            }
        } catch (_) {
            // Nothing left to report through; keep polling.
        }
    } finally {
        bridge.getNextCommand();
    }
};

cmds["GET_WINDOW_LIST"] = () => ({
    windows: kwin.getWindows().map(mapWindow),
});
cmds["GET_WINDOW"] = (params) => ({
    window: (() => {
        const window = kwin.getWindowByInternalId(params.internalId);
        return window ? mapWindow(window) : null;
    })(),
});
cmds["GET_ACTIVE_WINDOW"] = () => ({
    window: (() => {
        const window = kwin.getActiveWindow();
        return window ? mapWindow(window) : null;
    })(),
});
cmds["GET_CURSOR_POSITION"] = () => ({
    position: (() => {
        const position =
            typeof workspace.cursorPos === "function" ? workspace.cursorPos() : workspace.cursorPos;
        if (!position || typeof position.x !== "number" || typeof position.y !== "number") {
            return null;
        }
        return { x: position.x, y: position.y };
    })(),
});
cmds["GET_SCREENS"] = () => ({
    // Array.from also accepts the array-like list wrapper QJSEngine may hand out.
    screens: Array.from(workspace.screens || [], (output) => ({
        name: String(output.name || ""),
        x: Math.round(output.geometry.x),
        y: Math.round(output.geometry.y),
        width: Math.round(output.geometry.width),
        height: Math.round(output.geometry.height),
    })),
});

// Move and resize stay separate frameGeometry assignments: see resizeTo.
const moveTo = (window, x, y) => {
    const geometry = Object.assign({}, window.frameGeometry);
    geometry.x = x;
    geometry.y = y;
    window.frameGeometry = geometry;
};
const resizeTo = (window, width, height) => {
    const geometry = Object.assign({}, window.frameGeometry);
    // KWin snaps logical sizes to the output's device pixels, so on fractional scales the
    // frame differs from the requested size by less than a pixel. Re-requesting that size
    // sends a configure whose late ack restores the position it was sent with, which leaves
    // the window stuck at an animation frame partway across screens.
    if (Math.abs(geometry.width - width) < 1 && Math.abs(geometry.height - height) < 1) {
        return;
    }
    geometry.width = width;
    geometry.height = height;
    window.frameGeometry = geometry;
};
const setOpacity = (window, value) => {
    const opacity = Math.max(0, Math.min(1, Number(value)));
    if (Number.isNaN(opacity)) {
        throw new Error("window opacity must be numeric");
    }
    window.opacity = opacity;
};

cmds["MOVE_WINDOW"] = (params) => {
    moveTo(kwin.getWindowByInternalIdRequired(params.internalId), params.x, params.y);
    return {};
};
cmds["RESIZE_WINDOW"] = (params) => {
    resizeTo(kwin.getWindowByInternalIdRequired(params.internalId), params.width, params.height);
    return {};
};
// One animation frame in one round trip: move, then resize, then opacity, each optional.
cmds["SET_WINDOW_STATE"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    if (typeof params.x === "number" && typeof params.y === "number") {
        moveTo(window, params.x, params.y);
    }
    if (typeof params.width === "number" && typeof params.height === "number") {
        resizeTo(window, params.width, params.height);
    }
    if (params.opacity !== undefined && params.opacity !== null) {
        setOpacity(window, params.opacity);
    }
    return {};
};
cmds["SET_WINDOW_NO_BORDER"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    window.noBorder = Boolean(params.noBorder);
    return {};
};
// KWin pins a maximized or fullscreen window's size, so placing one only moves it. On
// Wayland both changes complete once the app acknowledges them. Fullscreen is left before
// and entered after the maximize change, so the maximize applies to the normal window.
cmds["SET_WINDOW_MODES"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    const vertically = Boolean(params.maximizedVertically);
    const horizontally = Boolean(params.maximizedHorizontally);
    const fullScreen = Boolean(params.fullScreen);
    if (!fullScreen) {
        window.fullScreen = false;
    }
    window.setMaximize(vertically, horizontally);
    if (fullScreen) {
        window.fullScreen = true;
    }
    return {};
};
// Pending LEAVE_WINDOW_MODES replies by window id, each a function that drops the wait.
const modeLeaves = {};
// Longer than the daemon waits; an older wait is dropped on its window's next signal.
const MODE_LEAVE_EXPIRY_MS = 2000;
const isNormal = (window) => !window.maximizeMode && !window.fullScreen;
const sameRect = (a, b) =>
    Math.round(a.x) === Math.round(b.x) &&
    Math.round(a.y) === Math.round(b.y) &&
    Math.round(a.width) === Math.round(b.width) &&
    Math.round(a.height) === Math.round(b.height);

// Leaves maximized and fullscreen state and answers once the app has acknowledged it: the
// state is normal and the window has left the pinned rect. Both are reported separately,
// in either order. The bridge keeps polling meanwhile; the daemon stops waiting after its
// own timeout, and a later answer is dropped.
cmds["LEAVE_WINDOW_MODES"] = (params, cmdInfo) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    if (isNormal(window)) {
        return { window: mapWindow(window) };
    }
    const id = params.internalId;
    if (modeLeaves[id]) {
        modeLeaves[id]();
    }
    // Copied field by field: a QRectF's fields need not be own enumerable properties.
    const g = window.frameGeometry;
    const pinned = { x: g.x, y: g.y, width: g.width, height: g.height };
    const expiresAt = Date.now() + MODE_LEAVE_EXPIRY_MS;
    // Answers at most once, even when X11 applies the change synchronously inside a setter.
    let done = false;
    const check = () => {
        if (done) {
            return;
        }
        if (Date.now() > expiresAt) {
            stop();
        } else if (isNormal(window) && !sameRect(window.frameGeometry, pinned)) {
            stop();
            bridge.sendResponse(cmdInfo, { window: mapWindow(window) }, null);
        }
    };
    const removed = (closed) => {
        if (!done && closed === window) {
            stop();
            bridge.sendResponse(cmdInfo, { window: null }, null);
        }
    };
    const stop = () => {
        done = true;
        window.maximizedChanged.disconnect(check);
        window.fullScreenChanged.disconnect(check);
        window.frameGeometryChanged.disconnect(check);
        workspace.windowRemoved.disconnect(removed);
        delete modeLeaves[id];
    };
    window.maximizedChanged.connect(check);
    window.fullScreenChanged.connect(check);
    window.frameGeometryChanged.connect(check);
    workspace.windowRemoved.connect(removed);
    modeLeaves[id] = stop;
    try {
        window.fullScreen = false;
        window.setMaximize(false, false);
    } catch (error) {
        // onGotCommand answers with the error; nothing may answer later.
        stop();
        throw error;
    }
    // X11 applies both at once.
    check();
    return bridge.DEFERRED;
};
// Moves a window to the named output; KWin refits a fullscreen window to it.
cmds["SEND_WINDOW_TO_SCREEN"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    workspace.sendClientToScreen(window, kwin.getScreenByNameRequired(params.screen));
    return {};
};
cmds["MINIMIZE_WINDOW"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    window.minimized = Boolean(params.minimized);
    return {};
};
cmds["BRING_WINDOW_TO_FOREGROUND"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    kwin.setActiveWindow(window);
    return {};
};
cmds["MOVE_WINDOW_TO_CURRENT_DESKTOP"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    const currentDesktop = workspace.currentDesktop;
    if (currentDesktop) {
        window.desktops = [currentDesktop];
    }
    return {};
};
cmds["REGISTER_HOT_KEY"] = (params) => {
    registerShortcut(params.name, params.title, params.sequence, () => {
        callDBus(
            bridge.DBUS_SERVICE,
            bridge.DBUS_PATH,
            bridge.DBUS_INTERFACE,
            "OnPressShortcut",
            params.name,
            "",
            "",
            ""
        );
    });
    return {};
};

const activationSignal = workspace.windowActivated || workspace.clientActivated;
if (activationSignal) {
    activationSignal.connect(bridge.onFocusChanged);
}

bridge.getNextCommand();
