<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  MemioClient,
  detectPlatform,
  readMemioSharedBuffer,
  memioUploadFile,
} from "memio-client";

const memio = new MemioClient();
const platform = ref(detectPlatform());
const memioReady = ref(false);

const backIpcTime = ref<string>("-");
const backMemioTime = ref<string>("-");

const uploadFile = ref<File | null>(null);
const frontIpcTime = ref<string>("-");
const frontMemioTime = ref<string>("-");

onMounted(async () => {
  const ready = await memio.waitForSharedMemory("file_transfer", 3000);
  memioReady.value = !!ready;
});

async function backFrontIpc() {
  const start = performance.now();
  await invoke("load_file_via_ipc");
  const elapsed = performance.now() - start;
  backIpcTime.value = `${elapsed.toFixed(2)} ms`;
}

async function backFrontMemio() {
  if (!memioReady.value) {
    backMemioTime.value = "Memio-rs not available";
    return;
  }

  const start = performance.now();
  await invoke("write_file_to_memio");
  const result = await readMemioSharedBuffer("file_transfer");
  if (!result) {
    backMemioTime.value = "Failed";
    return;
  }
  const elapsed = performance.now() - start;
  backMemioTime.value = `${elapsed.toFixed(2)} ms`;
}

async function frontBackIpc() {
  if (!uploadFile.value) {
    frontIpcTime.value = "Select a file";
    return;
  }

  const start = performance.now();
  const buffer = await uploadFile.value.arrayBuffer();
  const data = Array.from(new Uint8Array(buffer));
  await invoke("upload_file_ipc", { data });
  const elapsed = performance.now() - start;
  frontIpcTime.value = `${elapsed.toFixed(2)} ms`;
}

async function frontBackMemio() {
  if (!uploadFile.value) {
    frontMemioTime.value = "Select a file";
    return;
  }

  const start = performance.now();
  await memioUploadFile("upload", uploadFile.value);
  await invoke("read_upload");
  const elapsed = performance.now() - start;
  frontMemioTime.value = `${elapsed.toFixed(2)} ms`;
}
</script>

<template>
  <main class="container">
    <header>
      <p class="eyebrow">Memio-rs</p>
      <h1>Memio Tauri Example</h1>
      <p class="status">
        Platform: <strong>{{ platform }}</strong>
        <span :class="memioReady ? 'ok' : 'no'">
          {{ memioReady ? "Memio-rs ready" : "Memio-rs not available" }}
        </span>
      </p>
    </header>

    <section class="card">
      <h2>Rust → WebView</h2>
      <p>Excel file from public/.</p>
      <div class="row">
        <button type="button" @click="backFrontIpc">IPC</button>
        <button type="button" @click="backFrontMemio">Memio</button>
      </div>
      <div class="row">
        <div class="metric">IPC: <strong>{{ backIpcTime }}</strong></div>
        <div class="metric">Memio: <strong>{{ backMemioTime }}</strong></div>
      </div>
    </section>

    <section class="card">
      <h2>WebView → Rust</h2>
      <input type="file" @change="(e) => (uploadFile = (e.target as HTMLInputElement).files?.[0] || null)" />
      <div class="row">
        <button type="button" @click="frontBackIpc">IPC</button>
        <button type="button" @click="frontBackMemio">Memio</button>
      </div>
      <div class="row">
        <div class="metric">IPC: <strong>{{ frontIpcTime }}</strong></div>
        <div class="metric">Memio: <strong>{{ frontMemioTime }}</strong></div>
      </div>
    </section>
  </main>
</template>

<style scoped>
.container {
  max-width: 720px;
  margin: 0 auto;
  padding: 2.5rem 1.5rem 3rem;
  font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
  color: #1b1b1b;
}

header {
  margin-bottom: 2rem;
}

.eyebrow {
  text-transform: uppercase;
  letter-spacing: 0.18em;
  font-size: 0.7rem;
  color: #6b6b6b;
  margin: 0 0 0.5rem;
}

h1 {
  margin: 0 0 0.5rem;
  font-size: 2.2rem;
}

.status {
  margin: 0;
  color: #4a4a4a;
}

.ok {
  color: #0d7a4b;
  margin-left: 0.5rem;
}

.no {
  color: #b00020;
  margin-left: 0.5rem;
}

.card {
  background: #f7f4ef;
  border-radius: 16px;
  padding: 1.5rem;
  margin-bottom: 1.5rem;
  box-shadow: 0 16px 30px rgba(0, 0, 0, 0.08);
}

.card h2 {
  margin-top: 0;
}

button {
  border: none;
  border-radius: 999px;
  padding: 0.7rem 1.4rem;
  background: #1d1b21;
  color: #f6f2ec;
  font-weight: 600;
  cursor: pointer;
}

input {
  width: 100%;
  max-width: 420px;
  padding: 0.6rem 0.8rem;
  border-radius: 10px;
  border: 1px solid #d5cec5;
  margin: 0.6rem 0 1rem;
}

.row {
  display: flex;
  gap: 1rem;
  flex-wrap: wrap;
  align-items: center;
  margin-top: 0.75rem;
}

.metric {
  background: #1d1b21;
  color: #f6f2ec;
  padding: 0.5rem 0.9rem;
  border-radius: 999px;
  font-family: "IBM Plex Mono", "SFMono-Regular", monospace;
}
</style>
