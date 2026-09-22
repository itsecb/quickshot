import { mount } from "svelte";
import Editor from "./Editor.svelte";
import "./editor.css";

mount(Editor, { target: document.getElementById("app")! });
