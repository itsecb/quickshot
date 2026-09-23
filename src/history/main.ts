import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import History from "./History.svelte";
import "./history.css";

mount(History, { target: document.getElementById("app")! });
showWhenReady();
