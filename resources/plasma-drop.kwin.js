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
        const params = cmd(cmdInfo.params || {}) || {};
        bridge.sendResponse(cmdInfo, params, null);
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
