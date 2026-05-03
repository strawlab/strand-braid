function getPlotly() {
    if (!window.Plotly) {
        throw new Error("Plotly.js is not loaded");
    }
    return window.Plotly;
}

const cameraPlotIds = new Set();
let syncingCameraPlots = false;
let pendingLinkedFrameUpdate = null;
let linkedFrameAnimationId = 0;

function linkedFrameUpdateFromEvent(eventData) {
    if (eventData["xaxis.autorange"]) {
        return { "xaxis.autorange": true };
    }

    if (Array.isArray(eventData["xaxis.range"])) {
        return { "xaxis.range": eventData["xaxis.range"] };
    }

    if (
        eventData["xaxis.range[0]"] !== undefined
        && eventData["xaxis.range[1]"] !== undefined
    ) {
        return {
            "xaxis.range": [
                eventData["xaxis.range[0]"],
                eventData["xaxis.range[1]"],
            ],
        };
    }

    if (
        eventData["xaxis._rangeInitial[0]"] !== undefined
        && eventData["xaxis._rangeInitial[1]"] !== undefined
    ) {
        return {
            "xaxis.range": [
                eventData["xaxis._rangeInitial[0]"],
                eventData["xaxis._rangeInitial[1]"],
            ],
        };
    }

    return null;
}

function applyLinkedFrameUpdate(Plotly, sourceId, update) {
    pendingLinkedFrameUpdate = { Plotly, sourceId, update };
    if (linkedFrameAnimationId) {
        return;
    }

    linkedFrameAnimationId = requestAnimationFrame(() => {
        linkedFrameAnimationId = 0;
        const pending = pendingLinkedFrameUpdate;
        pendingLinkedFrameUpdate = null;
        if (!pending || syncingCameraPlots) {
            return;
        }

        syncingCameraPlots = true;
        const updates = [];
        cameraPlotIds.forEach((otherId) => {
            if (otherId === pending.sourceId) {
                return;
            }
            const other = document.getElementById(otherId);
            if (other) {
                updates.push(pending.Plotly.relayout(other, pending.update));
            }
        });
        Promise.allSettled(updates).finally(() => {
            syncingCameraPlots = false;
        });
    });
}

function installLinkedFrameZoom(Plotly, node, containerId) {
    if (node.dataset.linkedFrameZoom === "true") {
        return;
    }

    cameraPlotIds.add(containerId);
    node.dataset.linkedFrameZoom = "true";
    const onRangeChange = (eventData) => {
        if (syncingCameraPlots) {
            return;
        }

        const update = linkedFrameUpdateFromEvent(eventData);
        if (!update) {
            return;
        }

        applyLinkedFrameUpdate(Plotly, containerId, update);
    };

    node.on("plotly_relayouting", onRangeChange);
    node.on("plotly_relayout", onRangeChange);
}

export function clearPlot(containerId) {
    const node = document.getElementById(containerId);
    if (node && window.Plotly) {
        window.Plotly.purge(node);
        node.innerHTML = "";
    }
}

export function plotCamera2d(containerId, frames, xs, ys, title) {
    const Plotly = getPlotly();
    const node = document.getElementById(containerId);
    if (!node) {
        throw new Error(`Plot container not found: ${containerId}`);
    }

    const layout = {
        title: { text: title, font: { size: 14 } },
        margin: { t: 42, r: 18, b: 42, l: 52 },
        paper_bgcolor: "rgba(0,0,0,0)",
        plot_bgcolor: "rgba(0,0,0,0)",
        legend: { orientation: "h", x: 0, y: 1.15 },
        hovermode: "closest",
        dragmode: "pan",
        xaxis: {
            title: "Frame",
            showgrid: true,
            zeroline: false,
        },
        yaxis: {
            title: "Pixel",
            showgrid: true,
            zeroline: false,
        },
    };

    const config = {
        responsive: true,
        displaylogo: false,
        modeBarButtonsToRemove: ["lasso2d", "select2d"],
    };

    Plotly.react(
        node,
        [
            {
                x: frames,
                y: xs,
                name: "x",
                type: "scattergl",
                mode: "markers",
                marker: { color: "#d85050", size: 4, opacity: 0.75 },
                hovertemplate: "frame %{x}<br>x %{y:.2f}<extra></extra>",
            },
            {
                x: frames,
                y: ys,
                name: "y",
                type: "scattergl",
                mode: "markers",
                marker: { color: "#3a9d68", size: 4, opacity: 0.75 },
                hovertemplate: "frame %{x}<br>y %{y:.2f}<extra></extra>",
            },
        ],
        layout,
        config,
    );
    installLinkedFrameZoom(Plotly, node, containerId);
}
