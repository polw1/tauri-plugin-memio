import { createApp } from "vue";
import App from "./App.vue";
import "./style.css";
import { bootstrapWindowsSharedBuffer } from "memio-client";

// Initialize the SharedBuffer listener as early as possible on Windows
bootstrapWindowsSharedBuffer();

createApp(App).mount("#app");
