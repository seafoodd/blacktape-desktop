(devOrigin, startSentry) => {
    const { protocol, host, origin } = window.location;
    const packagedApp = (protocol === "tauri:" && host === "localhost")
        || ((protocol === "http:" || protocol === "https:") && host === "tauri.localhost");
    if (packagedApp || (devOrigin && origin === devOrigin)) {
        startSentry();
    }
}
