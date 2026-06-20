let viewer = null;

function getThree() {
    if (!window.THREE) {
        throw new Error("Three.js is not loaded");
    }
    return window.THREE;
}

function trajectoryColor(THREE, index) {
    const hue = (index * 137.508) % 360;
    return new THREE.Color(`hsl(${hue}, 68%, 54%)`);
}

function clearViewer() {
    if (!viewer) {
        return;
    }
    cancelAnimationFrame(viewer.animationId);
    window.removeEventListener("resize", viewer.resizeHandler);
    viewer.renderer.dispose();
    viewer.container.innerHTML = "";
    viewer = null;
}

function extentCenter(lim) {
    return (lim[0] + lim[1]) / 2;
}

function extentSize(lim) {
    const size = Math.abs(lim[1] - lim[0]);
    return size > 0 ? size : 1;
}

function makePerspectiveCamera(THREE, width, height, center, maxExtent) {
    const camera = new THREE.PerspectiveCamera(45, width / height, 0.001, maxExtent * 200);
    camera.position.set(center.x + maxExtent * 1.4, center.y - maxExtent * 1.8, center.z + maxExtent * 1.1);
    camera.up.set(0, 0, 1);
    camera.lookAt(center);
    return camera;
}

function makeOrthographicCamera(THREE, width, height, center, maxExtent, preset) {
    const aspect = width / height;
    const halfHeight = maxExtent * 0.72;
    const halfWidth = halfHeight * aspect;
    const camera = new THREE.OrthographicCamera(
        -halfWidth,
        halfWidth,
        halfHeight,
        -halfHeight,
        -maxExtent * 200,
        maxExtent * 200,
    );

    if (preset === "top-xy") {
        camera.position.set(center.x, center.y, center.z + maxExtent * 3);
        camera.up.set(0, 1, 0);
    } else {
        camera.position.set(center.x, center.y - maxExtent * 3, center.z);
        camera.up.set(0, 0, 1);
    }
    camera.lookAt(center);
    return camera;
}

function cameraColor(THREE, index) {
    const hue = (index * 137.508 + 40) % 360;
    return new THREE.Color(`hsl(${hue}, 70%, 62%)`);
}

// World-space point for an image-plane pixel at forward distance `z`.
function pixelToWorld(THREE, cam, px, py, z) {
    const a = (px - cam.cx) / cam.fx;
    const b = (py - cam.cy) / cam.fy;
    return new THREE.Vector3(
        cam.center[0] + z * (a * cam.right[0] + b * cam.down[0] + cam.forward[0]),
        cam.center[1] + z * (a * cam.right[1] + b * cam.down[1] + cam.forward[1]),
        cam.center[2] + z * (a * cam.right[2] + b * cam.down[2] + cam.forward[2]),
    );
}

function makeLabelSprite(THREE, text, color) {
    const pad = 8;
    const font = "28px sans-serif";
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d");
    ctx.font = font;
    const textWidth = Math.ceil(ctx.measureText(text).width);
    canvas.width = textWidth + pad * 2;
    canvas.height = 28 + pad * 2;
    ctx.font = font;
    ctx.fillStyle = "rgba(20, 24, 33, 0.72)";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = `#${color.getHexString()}`;
    ctx.textBaseline = "top";
    ctx.fillText(text, pad, pad);

    const texture = new THREE.CanvasTexture(canvas);
    texture.minFilter = THREE.LinearFilter;
    const material = new THREE.SpriteMaterial({ map: texture, transparent: true, depthTest: false });
    const sprite = new THREE.Sprite(material);
    sprite.userData.aspect = canvas.width / canvas.height;
    return sprite;
}

// Draw camera frustums in the style of rerun's 3D view: a rectangle at the
// image plane connected back to the camera center, plus an "up" triangle.
function drawCameras(THREE, scene, cameras, maxExtent) {
    if (!cameras || cameras.length === 0) {
        return;
    }
    const z = maxExtent * 0.16;
    const labelScale = maxExtent * 0.12;

    cameras.forEach((cam, index) => {
        if (!Number.isFinite(cam.fx) || !Number.isFinite(cam.fy) || cam.fx === 0 || cam.fy === 0) {
            return;
        }
        const color = cameraColor(THREE, index);
        const apex = new THREE.Vector3(cam.center[0], cam.center[1], cam.center[2]);
        const w = cam.width;
        const h = cam.height;

        const c0 = pixelToWorld(THREE, cam, 0, 0, z);
        const c1 = pixelToWorld(THREE, cam, 0, h, z);
        const c2 = pixelToWorld(THREE, cam, w, h, z);
        const c3 = pixelToWorld(THREE, cam, w, 0, z);

        const material = new THREE.LineBasicMaterial({ color, linewidth: 2 });

        // Frustum edges: rectangle corners tied back to the camera center.
        const frustum = [c0, c1, apex, c2, c3, apex, c0, c3, apex];
        scene.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints(frustum), material));
        // Remaining far-plane edge.
        scene.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints([c1, c2]), material));

        // Up-direction triangle above the image-plane top edge.
        const t0 = pixelToWorld(THREE, cam, 0.4 * w, 0, z);
        const t1 = pixelToWorld(THREE, cam, 0.5 * w, -0.1 * w, z);
        const t2 = pixelToWorld(THREE, cam, 0.6 * w, 0, z);
        scene.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints([t0, t1, t2]), material));

        if (cam.name) {
            const label = makeLabelSprite(THREE, cam.name, color);
            label.position.copy(apex);
            label.scale.set(labelScale * label.userData.aspect, labelScale, 1);
            scene.add(label);
        }
    });
}

export function renderTrajectories3d(containerId, trajectories, cameras, bounds) {
    const THREE = getThree();
    const container = document.getElementById(containerId);
    if (!container) {
        throw new Error(`3D container not found: ${containerId}`);
    }

    clearViewer();

    const initialWidth = Math.max(container.clientWidth, 320);
    const initialHeight = Math.max(container.clientHeight, 320);
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(window.devicePixelRatio || 1);
    renderer.setClearColor(0x000000, 0);
    renderer.setSize(initialWidth, initialHeight);
    renderer.domElement.style.display = "block";
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.height = "100%";
    container.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const center = new THREE.Vector3(
        extentCenter(bounds.x),
        extentCenter(bounds.y),
        extentCenter(bounds.z),
    );
    const maxExtent = Math.max(extentSize(bounds.x), extentSize(bounds.y), extentSize(bounds.z));

    const cameraRef = {
        camera: makePerspectiveCamera(THREE, initialWidth, initialHeight, center, maxExtent),
    };
    const controls = createOrbitControls(THREE, cameraRef, renderer.domElement, center, maxExtent);

    scene.add(new THREE.AmbientLight(0xffffff, 0.92));

    const grid = new THREE.GridHelper(maxExtent * 1.35, 10, 0x6f7d91, 0xc3ccd8);
    grid.rotation.x = Math.PI / 2;
    grid.position.copy(center);
    grid.position.z = bounds.z[0];
    scene.add(grid);

    const axes = new THREE.AxesHelper(maxExtent * 0.45);
    axes.position.copy(center);
    scene.add(axes);

    drawCameras(THREE, scene, cameras, maxExtent);

    trajectories.forEach((traj, index) => {
        const count = Math.min(traj.x.length, traj.y.length, traj.z.length);
        if (count < 2) {
            return;
        }

        const points = [];
        for (let i = 0; i < count; i += 1) {
            points.push(new THREE.Vector3(traj.x[i], traj.y[i], traj.z[i]));
        }

        const geometry = new THREE.BufferGeometry().setFromPoints(points);
        const material = new THREE.LineBasicMaterial({
            color: trajectoryColor(THREE, index),
            linewidth: 2,
        });
        scene.add(new THREE.Line(geometry, material));
    });

    function onResize() {
        if (!viewer) {
            return;
        }
        const width = Math.max(container.clientWidth, 320);
        const height = Math.max(container.clientHeight, 320);
        resizeCamera(cameraRef.camera, width, height, maxExtent);
        renderer.setSize(width, height);
    }

    function animate() {
        controls.update();
        renderer.render(scene, cameraRef.camera);
        viewer.animationId = requestAnimationFrame(animate);
    }

    viewer = {
        animationId: 0,
        bounds,
        cameraRef,
        center,
        container,
        controls,
        maxExtent,
        preset: "free",
        projection: "perspective",
        renderer,
        resizeHandler: onResize,
        scene,
    };
    window.addEventListener("resize", onResize, { passive: true });
    setTrajectoryViewStatus("Free view, perspective");
    animate();
}

export function setTrajectoryView(preset) {
    if (!viewer) {
        throw new Error("3D viewer is not initialized");
    }

    const THREE = getThree();
    const width = Math.max(viewer.container.clientWidth, 320);
    const height = Math.max(viewer.container.clientHeight, 320);
    let label;
    let projection;

    if (preset === "top-xy") {
        viewer.cameraRef.camera = makeOrthographicCamera(
            THREE,
            width,
            height,
            viewer.center,
            viewer.maxExtent,
            "top-xy",
        );
        label = "Top-view (XY), orthographic";
        projection = "orthographic";
    } else if (preset === "side-xz") {
        viewer.cameraRef.camera = makeOrthographicCamera(
            THREE,
            width,
            height,
            viewer.center,
            viewer.maxExtent,
            "side-xz",
        );
        label = "Side-view (XZ), orthographic";
        projection = "orthographic";
    } else if (preset === "free") {
        viewer.cameraRef.camera = makePerspectiveCamera(
            THREE,
            width,
            height,
            viewer.center,
            viewer.maxExtent,
        );
        label = "Free view, perspective";
        projection = "perspective";
    } else {
        throw new Error(`Unknown 3D view preset: ${preset}`);
    }

    viewer.preset = preset;
    viewer.projection = projection;
    viewer.controls.resetTarget(viewer.center);
    viewer.controls.syncFromCamera();
    setTrajectoryViewStatus(label, preset);
}

function resizeCamera(camera, width, height, maxExtent) {
    if (camera.isOrthographicCamera) {
        const aspect = width / height;
        const halfHeight = maxExtent * 0.72;
        const halfWidth = halfHeight * aspect;
        camera.left = -halfWidth;
        camera.right = halfWidth;
        camera.top = halfHeight;
        camera.bottom = -halfHeight;
    } else {
        camera.aspect = width / height;
    }
    camera.updateProjectionMatrix();
}

function setTrajectoryViewStatus(text, preset = "free") {
    const node = document.getElementById("trajectory-view-status");
    if (node) {
        node.textContent = text;
    }
    document.querySelectorAll("[data-view-preset]").forEach((button) => {
        button.classList.toggle("is-active", button.dataset.viewPreset === preset);
    });
}

function createOrbitControls(THREE, cameraRef, domElement, initialTarget, sceneScale) {
    const target = initialTarget.clone();
    const spherical = new THREE.Spherical();
    const offset = new THREE.Vector3();
    const panOffset = new THREE.Vector3();
    const rotateDelta = new THREE.Vector2();
    const damping = 0.82;
    let state = "none";
    let lastX = 0;
    let lastY = 0;

    // THREE.Spherical assumes a Y-up world, but our cameras are Z-up (see
    // makePerspectiveCamera). Rotate offsets into a Y-up frame before converting
    // to/from spherical so horizontal drags orbit around the scene's up axis.
    const yUp = new THREE.Vector3(0, 1, 0);
    function upToYQuaternion() {
        return new THREE.Quaternion().setFromUnitVectors(cameraRef.camera.up, yUp);
    }

    function updateSphericalFromCamera() {
        offset.copy(cameraRef.camera.position).sub(target);
        offset.applyQuaternion(upToYQuaternion());
        spherical.setFromVector3(offset);
        spherical.makeSafe();
    }

    function pan(pixelDx, pixelDy) {
        const distance = Math.max(spherical.radius, sceneScale * 0.05);
        const scale = cameraRef.camera.isOrthographicCamera
            ? (cameraRef.camera.top - cameraRef.camera.bottom) / Math.max(domElement.clientHeight, 1)
            : (2 * distance * Math.tan((cameraRef.camera.fov * Math.PI / 180) / 2))
                / Math.max(domElement.clientHeight, 1);
        const xAxis = new THREE.Vector3().setFromMatrixColumn(cameraRef.camera.matrix, 0);
        const yAxis = new THREE.Vector3().setFromMatrixColumn(cameraRef.camera.matrix, 1);
        panOffset.addScaledVector(xAxis, -pixelDx * scale);
        panOffset.addScaledVector(yAxis, pixelDy * scale);
    }

    function orbit(pixelDx, pixelDy) {
        if (cameraRef.camera.isOrthographicCamera) {
            return;
        }
        rotateDelta.x -= (2 * Math.PI * pixelDx) / Math.max(domElement.clientWidth, 1);
        rotateDelta.y -= (Math.PI * pixelDy) / Math.max(domElement.clientHeight, 1);
    }

    function applyZoom(deltaY) {
        const zoom = Math.exp(deltaY * 0.001);
        if (cameraRef.camera.isOrthographicCamera) {
            cameraRef.camera.zoom = Math.max(0.05, Math.min(50, cameraRef.camera.zoom / zoom));
            cameraRef.camera.updateProjectionMatrix();
        } else {
            spherical.radius = Math.max(sceneScale * 0.01, spherical.radius * zoom);
        }
    }

    function onPointerDown(event) {
        const wantsOrbit = event.button === 1 || (event.button === 0 && event.altKey && !event.shiftKey);
        const wantsPan = (event.button === 1 && event.shiftKey)
            || (event.button === 0 && event.altKey && event.shiftKey);

        if (!wantsOrbit && !wantsPan) {
            return;
        }

        domElement.setPointerCapture(event.pointerId);
        state = wantsPan ? "pan" : "rotate";
        lastX = event.clientX;
        lastY = event.clientY;
        event.preventDefault();
    }

    function onPointerMove(event) {
        if (state === "none") {
            return;
        }
        const dx = event.clientX - lastX;
        const dy = event.clientY - lastY;
        lastX = event.clientX;
        lastY = event.clientY;

        if (state === "pan") {
            pan(dx, dy);
        } else {
            orbit(dx, dy);
        }
        event.preventDefault();
    }

    function onPointerUp(event) {
        if (domElement.hasPointerCapture(event.pointerId)) {
            domElement.releasePointerCapture(event.pointerId);
        }
        state = "none";
    }

    // Trackpad gestures and mouse wheels both arrive as `wheel` events. Once we
    // have positive evidence of a trackpad we route plain two-finger drags to
    // orbit/pan (Blender style) instead of zoom. Pinch-zoom always zooms.
    let trackpadDetected = false;

    function onWheel(event) {
        event.preventDefault();

        // Browsers report a trackpad pinch-zoom as a wheel event with ctrlKey
        // set (even when Ctrl is not pressed). Treat it — and a real Ctrl+wheel
        // — as zoom.
        if (event.ctrlKey) {
            trackpadDetected = true;
            applyZoom(event.deltaY);
            return;
        }

        // Line/page-mode wheel events only come from real mouse wheels.
        if (event.deltaMode !== 0) {
            applyZoom(event.deltaY);
            return;
        }

        // A horizontal component or fractional deltas only occur on trackpads.
        if (event.deltaX !== 0 || !Number.isInteger(event.deltaY)) {
            trackpadDetected = true;
        }

        if (!trackpadDetected) {
            // Looks like a classic mouse wheel: keep zoom-on-scroll.
            applyZoom(event.deltaY);
            return;
        }

        // Trackpad two-finger drag (Blender style): orbit, or pan with Shift.
        // Negate so the view tracks the direction of finger motion.
        const dx = -event.deltaX;
        const dy = -event.deltaY;
        if (event.shiftKey || cameraRef.camera.isOrthographicCamera) {
            pan(dx, dy);
        } else {
            orbit(dx, dy);
        }
    }

    domElement.addEventListener("contextmenu", (event) => event.preventDefault());
    domElement.addEventListener("pointerdown", onPointerDown);
    domElement.addEventListener("pointermove", onPointerMove);
    domElement.addEventListener("pointerup", onPointerUp);
    domElement.addEventListener("pointercancel", onPointerUp);
    domElement.addEventListener("wheel", onWheel, { passive: false });

    updateSphericalFromCamera();

    return {
        resetTarget(newTarget) {
            target.copy(newTarget);
            panOffset.set(0, 0, 0);
            rotateDelta.set(0, 0);
        },
        syncFromCamera: updateSphericalFromCamera,
        update() {
            if (!cameraRef.camera.isOrthographicCamera) {
                spherical.theta += rotateDelta.x;
                spherical.phi += rotateDelta.y;
                spherical.makeSafe();
            }
            if (cameraRef.camera.isOrthographicCamera) {
                cameraRef.camera.position.add(panOffset);
                target.add(panOffset);
            } else {
                target.add(panOffset);
                offset.setFromSpherical(spherical);
                offset.applyQuaternion(upToYQuaternion().invert());
                cameraRef.camera.position.copy(target).add(offset);
            }
            cameraRef.camera.lookAt(target);

            rotateDelta.multiplyScalar(damping);
            panOffset.multiplyScalar(damping);
            if (Math.abs(rotateDelta.x) < 0.00001) {
                rotateDelta.x = 0;
            }
            if (Math.abs(rotateDelta.y) < 0.00001) {
                rotateDelta.y = 0;
            }
            if (panOffset.lengthSq() < 0.0000001) {
                panOffset.set(0, 0, 0);
            }
        },
    };
}
