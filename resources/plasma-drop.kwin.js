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

bridge.onFocusChanged = () => {
    callDBus(
        bridge.DBUS_SERVICE,
        bridge.DBUS_PATH,
        bridge.DBUS_INTERFACE,
        "OnFocusChanged"
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

bridge.onGotCommand = (cmdInfoStr) => {
    const cmdInfo = JSON.parse(cmdInfoStr);
    try {
        const cmd = cmds[cmdInfo.type];
        if (typeof cmd !== "function") {
            throw new Error(`Unknown command '${cmdInfo.type}'`);
        }
        const params = cmd(cmdInfo.params || {}) || {};
        bridge.sendResponse(cmdInfo, params, null);
    } catch (error) {
        log.error(String(error && error.message ? error.message : error));
        bridge.sendResponse(cmdInfo, {}, String(error && error.message ? error.message : error));
    }
    bridge.getNextCommand();
};

cmds["NOOP"] = () => ({});
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
        try {
            // Engana o Rust informando que o mouse está sempre dentro do HDMI
            const hdmiScreen = workspace.screens.find(s => s.name === "HDMI-A-1");
            if (hdmiScreen) {
                return { x: hdmiScreen.geometry.x + 10, y: hdmiScreen.geometry.y + 10 };
            }
        } catch (e) {}
        
        // Fallback original caso a tela não seja encontrada
        const position = typeof workspace.cursorPos === "function" ? workspace.cursorPos() : workspace.cursorPos;
        if (!position || typeof position.x !== "number" || typeof position.y !== "number") {
            return null;
        }
        return { x: position.x, y: position.y };
    })(),
});
cmds["GET_SUPPORT_INFORMATION"] = () => ({
    text: workspace.supportInformation(),
});
cmds["MOVE_WINDOW"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    
    try {
        if (workspace.screens) {
            const targetScreen = workspace.screens.find(s => 
                params.x >= s.geometry.x && params.x < (s.geometry.x + s.geometry.width) &&
                params.y >= s.geometry.y && params.y < (s.geometry.y + s.geometry.height)
            );
            if (targetScreen) {
                if (typeof workspace.sendWindowToOutput === "function") {
                    workspace.sendWindowToOutput(window, targetScreen);
                } else if (typeof workspace.sendClientToScreen === "function") {
                    workspace.sendClientToScreen(window, targetScreen);
                }
            }
        }
    } catch (e) {}

    const geometry = Object.assign({}, window.frameGeometry);
    geometry.x = params.x;
    geometry.y = params.y;
    window.frameGeometry = geometry;
    return {};
};
cmds["RESIZE_WINDOW"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    const geometry = Object.assign({}, window.frameGeometry);
    geometry.width = params.width;
    geometry.height = params.height;
    window.frameGeometry = geometry;
    return {};
};
cmds["SET_WINDOW_OPACITY"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    const opacity = Math.max(0, Math.min(1, Number(params.opacity)));
    if (Number.isNaN(opacity)) {
        throw new Error("SET_WINDOW_OPACITY requires a numeric opacity");
    }
    window.opacity = opacity;
    return {};
};
cmds["SET_WINDOW_NO_BORDER"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    window.noBorder = Boolean(params.noBorder);
    return {};
};
cmds["MINIMIZE_WINDOW"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    
    // INTERCEPTAÇÃO: Se o programa mandou esconder, mas o Konsole não está com o foco atual
    if (workspace.activeWindow !== window) {
        window.minimized = false;           // Garante que a janela fica visível
        workspace.activeWindow = window;    // Puxa o foco do teclado para o Konsole
        return {};                          // Aborta a ordem de minimizar
    }

    // Se o Konsole já era a janela com foco, minimiza e esconde normalmente
    window.minimized = true;
    return {};
};
cmds["BRING_WINDOW_TO_FOREGROUND"] = (params) => {
    const window = kwin.getWindowByInternalIdRequired(params.internalId);
    window.minimized = false;
    workspace.activeWindow = window;
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
