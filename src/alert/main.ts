import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Alert from "./Alert.svelte";
import "../thumb/thumb.css";
import "./alert.css";

mount(Alert, { target: document.getElementById("app")! });
// like the capture thumbnail: never take focus from what the user is doing
showWhenReady({ focus: false });
