import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Thumb from "./Thumb.svelte";
import "./thumb.css";

mount(Thumb, { target: document.getElementById("app")! });
// never take focus: whatever the user was doing keeps the keyboard
showWhenReady({ focus: false });
