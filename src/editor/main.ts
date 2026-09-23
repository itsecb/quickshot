import { mount } from "svelte";
import { showWhenReady } from "$lib/window";
import Editor from "./Editor.svelte";
import "./editor.css";

mount(Editor, { target: document.getElementById("app")! });
showWhenReady();
