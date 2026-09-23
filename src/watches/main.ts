import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Watches from "./Watches.svelte";
import "./watches.css";

mount(Watches, { target: document.getElementById("app")! });
showWhenReady();
