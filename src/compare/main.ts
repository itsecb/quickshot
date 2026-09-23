import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Compare from "./Compare.svelte";
import "./compare.css";

mount(Compare, { target: document.getElementById("app")! });
showWhenReady();
