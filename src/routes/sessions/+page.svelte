<script lang="ts">
  import { onMount } from "svelte";
  import logotypeDark from "$lib/assets/logotype-dark.svg";

  type SessionInfo = {
    id: string;
    session_name: string;
    users: string[];
    shell_count: number;
    url: string;
  };

  let token = "";
  let sessions: SessionInfo[] = [];
  let error = "";
  let loading = false;
  let authenticated = false;

  const STORAGE_KEY = "sshx-admin-token";

  onMount(() => {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      token = saved;
      fetchSessions();
    }
  });

  async function fetchSessions() {
    loading = true;
    error = "";
    try {
      const res = await fetch(
        `/api/sessions?token=${encodeURIComponent(token)}`,
      );
      if (res.ok) {
        const data = await res.json();
        sessions = data.sessions;
        authenticated = true;
        localStorage.setItem(STORAGE_KEY, token);
      } else {
        const data = await res.json().catch(() => ({}));
        error = data.error || `HTTP ${res.status}`;
        authenticated = false;
        localStorage.removeItem(STORAGE_KEY);
      }
    } catch {
      error = "Network error";
    } finally {
      loading = false;
    }
  }

  function logout() {
    token = "";
    sessions = [];
    authenticated = false;
    localStorage.removeItem(STORAGE_KEY);
  }

  function handleSubmit() {
    if (token.trim()) fetchSessions();
  }
</script>

<svelte:head>
  <title>Admin - Sessions | sshx</title>
</svelte:head>

<main class="max-w-screen-lg mx-auto px-4 md:px-8 text-zinc-100 py-8">
  <header class="mb-8 flex items-center justify-between">
    <a href="/">
      <img class="h-10" src={logotypeDark} alt="sshx logo" />
    </a>
    {#if authenticated}
      <div class="flex gap-3">
        <button
          class="px-4 py-1.5 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-sm"
          on:click={fetchSessions}
          disabled={loading}
        >
          Refresh
        </button>
        <button
          class="px-4 py-1.5 rounded-lg bg-zinc-800 hover:bg-zinc-700 text-sm text-zinc-400"
          on:click={logout}
        >
          Logout
        </button>
      </div>
    {/if}
  </header>

  {#if !authenticated}
    <div class="max-w-sm mx-auto mt-24">
      <h2 class="text-xl font-medium mb-4">Admin Access</h2>
      <form on:submit|preventDefault={handleSubmit} class="space-y-4">
        <input
          type="password"
          bind:value={token}
          placeholder="Enter admin token"
          class="w-full px-4 py-2 rounded-lg bg-zinc-800 border border-zinc-700
                 focus:border-indigo-500 focus:outline-none text-zinc-100"
        />
        {#if error}
          <p class="text-red-400 text-sm">{error}</p>
        {/if}
        <button
          type="submit"
          class="w-full px-4 py-2 rounded-lg bg-indigo-700 hover:bg-indigo-600 font-medium"
          disabled={loading}
        >
          {loading ? "Checking..." : "Sign In"}
        </button>
      </form>
    </div>
  {:else}
    <h2 class="text-xl font-medium mb-4">
      Active Sessions
      <span class="text-zinc-500 text-base ml-2">({sessions.length})</span>
    </h2>

    {#if sessions.length === 0}
      <p class="text-zinc-500">No active sessions.</p>
    {:else}
      <div class="border border-zinc-800 rounded-xl overflow-hidden">
        <table class="w-full text-sm">
          <thead>
            <tr class="border-b border-zinc-800 text-zinc-400 text-left">
              <th class="px-4 py-3 font-medium">Session</th>
              <th class="px-4 py-3 font-medium">Users</th>
              <th class="px-4 py-3 font-medium">Shells</th>
              <th class="px-4 py-3 font-medium">Link</th>
            </tr>
          </thead>
          <tbody>
            {#each sessions as session}
              <tr class="border-b border-zinc-800/50 hover:bg-zinc-800/30">
                <td class="px-4 py-3">
                  <div class="font-medium">
                    {session.session_name || session.id}
                  </div>
                  <div class="text-xs text-zinc-500 font-mono">{session.id}</div>
                </td>
                <td class="px-4 py-3">
                  {#if session.users.length === 0}
                    <span class="text-zinc-600">--</span>
                  {:else}
                    {#each session.users as user}
                      <span
                        class="inline-block bg-zinc-800 rounded px-2 py-0.5 mr-1 mb-1 text-xs"
                      >
                        {user}
                      </span>
                    {/each}
                  {/if}
                </td>
                <td class="px-4 py-3 text-zinc-400">{session.shell_count}</td>
                <td class="px-4 py-3">
                  <a
                    href={session.url}
                    target="_blank"
                    rel="noopener"
                    class="text-indigo-400 hover:text-indigo-300 underline text-xs font-mono"
                  >
                    {session.url}
                  </a>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    <p class="mt-6 text-xs text-zinc-600">
      Only local sessions are shown. Session links require the encryption key
      (URL fragment) to access terminal content.
    </p>
  {/if}
</main>
