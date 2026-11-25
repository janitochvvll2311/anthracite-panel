// A tiny Docker Engine API fixture for the panel smoke test.
//
//   bun scripts/containers-fixture.ts [socket]
//
// Seeded with a running `web`, an exited `job`, an image, a volume and the
// bridge network. The panel e2e spec that expects them is gated by
// ANTH_CONTAINERS_FIXTURE=1.
import { chmodSync, mkdirSync } from "node:fs";

const socket =
  process.argv[2] ??
  process.env.ANTH_CONTAINERS_SOCKET ??
  "/tmp/anthracite-panel/containers-fixture/docker.sock";
mkdirSync(socket.slice(0, socket.lastIndexOf("/")), { recursive: true });
let next = 0;

type Container = {
  id: string;
  name: string;
  image: string;
  state: string;
  log: string;
  env: string[];
};

const containers = new Map<string, Container>([
  ["web", { id: "web", name: "web", image: "nginx:1.27", state: "running", log: "hello from the fixture\n", env: [] }],
  ["job", { id: "job", name: "job", image: "alpine:3.21", state: "exited", log: "done\n", env: [] }],
]);
const volumes = [{ Name: "seed", Driver: "local", Labels: {} }];

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function frame(text: string): Uint8Array {
  const payload = new TextEncoder().encode(text);
  const out = new Uint8Array(8 + payload.length);
  out[0] = 1;
  new DataView(out.buffer).setUint32(4, payload.length);
  out.set(payload, 8);
  return out;
}

function streamed(body: Uint8Array): Response {
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(body);
      controller.close();
    },
  });
  return new Response(stream, {
    headers: { "content-type": "application/octet-stream", "transfer-encoding": "chunked" },
  });
}

Bun.serve({
  unix: socket,
  async fetch(request) {
    const url = new URL(request.url);
    const path = url.pathname;
    const method = request.method;
    if (!path.startsWith("/v1.43/")) {
      if (method === "GET" && path === "/version") {
        return json({ ApiVersion: "1.43" });
      }
      return json({ message: "bad version" }, 404);
    }
    const rest = path.slice("/v1.43".length);
    const parts = rest.split("/").filter(Boolean);
    if (method === "GET" && rest === "/containers/json") {
      return json(
        [...containers.values()].map((container) => ({
          Id: container.id,
          Names: [`/${container.name}`],
          Image: container.image,
          State: container.state,
          Status: container.state === "running" ? "Up 2 minutes" : "Exited (0)",
          Created: 1_795_000_000,
          Ports: [],
          Labels: { "com.example": container.name },
        })),
      );
    }
    if (method === "POST" && rest === "/containers/create") {
      next += 1;
      const body = await request.json() as any;
      const id = `c${next}`;
      const name = url.searchParams.get("name") ?? id;
      containers.set(id, {
        id,
        name,
        image: body.Image ?? "",
        state: "created",
        log: "",
        env: body.Env ?? [],
      });
      return json({ Id: id }, 201);
    }
    if (parts[0] === "containers" && parts[1]) {
      const container = containers.get(parts[1]);
      if (!container) return json({ message: "no such container" }, 404);
      if (parts[2] === "json") {
        return json({
          Id: container.id,
          Name: `/${container.name}`,
          Created: "2026-11-25T12:00:00Z",
          Config: {
            Image: container.image,
            Tty: false,
            Labels: { "com.example": container.name },
            Env: container.env,
          },
          State: {
            Status: container.state,
            Health: container.state === "running" ? { Status: "healthy" } : undefined,
          },
          NetworkSettings: { Ports: {} },
        });
      }
      if (parts[2] === "logs") {
        return streamed(frame(container.log));
      }
      if (parts[2] === "stats") {
        return json({
          cpu_stats: {
            cpu_usage: { total_usage: 200 },
            system_cpu_usage: 1000,
            online_cpus: 2,
          },
          precpu_stats: {
            cpu_usage: { total_usage: 100 },
            system_cpu_usage: 500,
          },
          memory_stats: { usage: 1_048_576, limit: 4_194_304, stats: { cache: 0 } },
          networks: { eth0: { rx_bytes: 1024, tx_bytes: 2048 } },
          pids_stats: { current: 1 },
        });
      }
      if (parts[2] === "exec") {
        return json({ Id: "exec" }, 201);
      }
      if (
        method === "POST" &&
        ["start", "stop", "restart", "pause", "unpause", "kill"].includes(parts[2])
      ) {
        if (parts[2] === "stop" || parts[2] === "kill") container.state = "exited";
        else if (parts[2] === "pause") container.state = "paused";
        else container.state = "running";
        return new Response(null, { status: 204 });
      }
      if (method === "DELETE") {
        if (container.state === "running" && url.searchParams.get("force") !== "1") {
          return json({ message: "container is running" }, 409);
        }
        containers.delete(parts[1]);
        return new Response(null, { status: 204 });
      }
    }
    if (method === "GET" && rest === "/images/json") {
      return json([
        {
          Id: "sha256:nginx",
          RepoTags: ["nginx:1.27"],
          Size: 1000,
          Created: 10,
          RepoDigests: ["nginx@sha256:abc"],
        },
      ]);
    }
    if (method === "GET" && rest === "/volumes") {
      return json({ Volumes: volumes });
    }
    if (method === "POST" && rest === "/volumes/create") {
      const body = await request.json() as any;
      const volume = { Name: body.Name, Driver: "local", Labels: {} };
      volumes.push(volume);
      return json(volume, 201);
    }
    if (method === "DELETE" && parts[0] === "volumes" && parts[1]) {
      const index = volumes.findIndex((volume) => volume.Name === parts[1]);
      if (index >= 0) volumes.splice(index, 1);
      return new Response(null, { status: 204 });
    }
    if (method === "GET" && rest === "/networks") {
      return json([
        {
          Name: "bridge",
          Driver: "bridge",
          IPAM: { Config: [{ Subnet: "172.17.0.0/16" }] },
          Containers: { web: {} },
        },
      ]);
    }
    return json({ message: "not found" }, 404);
  },
});

// The service runs as a dedicated uid with the engine gid; the fixture socket
// is world-connectable so the approval check and the service both pass.
chmodSync(socket, 0o666);
console.log("docker fixture on " + socket);
