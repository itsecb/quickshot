import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Guide from "./Guide.svelte";
import "./guide.css";

mount(Guide, { target: document.getElementById("app")! });
showWhenReady();
