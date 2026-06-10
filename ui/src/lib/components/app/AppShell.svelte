<script lang="ts">
  import { onMount } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { createMutation, createQuery } from "@tanstack/svelte-query";
  import Login from "$lib/components/Login.svelte";
  import BucketList from "$lib/components/BucketList.svelte";
  import ObjectBrowser from "$lib/components/ObjectBrowser.svelte";
  import BucketSettings from "$lib/components/BucketSettings.svelte";
  import UserList from "$lib/components/UserList.svelte";
  import Metrics from "$lib/components/Metrics.svelte";
  import ServerSettings from "$lib/components/ServerSettings.svelte";
  import AppSidebar from "$lib/components/app/sidebar/AppSidebar.svelte";
  import { buildSidebarNavItems } from "$lib/components/app/sidebar/navigation";
  import {
    applyThemeToDocument,
    isThemeMode,
    nextThemeMode,
    type ThemeMode,
  } from "$lib/components/app/sidebar/theme";
  import { base } from "$app/paths";
  import ArrowLeft from "lucide-svelte/icons/arrow-left";
  import ChevronRight from "lucide-svelte/icons/chevron-right";
  import Menu from "lucide-svelte/icons/menu";
  import { Sonner } from "$lib/components/ui/sonner";
  import { checkAuth, logout, type AuthCheckResponse } from "$lib/api/auth";
  import { listBuckets } from "$lib/api/buckets";
  import { authKeys, bucketKeys } from "$lib/api/keys";
  import { formatBytes } from "$lib/format-bytes";
  import { queryClient } from "$lib/query/client";

  const authQuery = createQuery(() => ({
    queryKey: authKeys.check(),
    queryFn: checkAuth,
    retry: false,
  }));
  const logoutMutation = createMutation(() => ({
    mutationFn: logout,
    onSuccess: () => {
      queryClient.clear();
      queryClient.invalidateQueries({ queryKey: authKeys.all });
    },
  }));

  let authenticatedOverride = $state<boolean | null>(null);
  let sessionIsRoot = $state<boolean | null>(null);
  let collapsed = $state(false);
  let selectedBucket = $state<string | null>(null);
  let currentView = $state<"objects" | "settings" | "users" | "metrics" | "serverSettings">("objects");
  let objectBrowserRef = $state<ObjectBrowser | null>(null);
  let currentPrefix = $state("");
  let currentBreadcrumbs = $state<{ label: string; prefix: string }[]>([]);
  let themeMode = $state<ThemeMode>("system");
  let isDark = $state(true);
  let pendingPrefix = $state<string | null>(null);
  let mobileMenuOpen = $state(false);

  const bucketsQuery = createQuery(() => ({
    queryKey: bucketKeys.list(),
    queryFn: listBuckets,
    enabled: !!selectedBucket,
  }));

  const bucketStats = $derived(
    selectedBucket
      ? bucketsQuery.data?.buckets.find((b) => b.name === selectedBucket)
      : undefined,
  );

  $effect(() => {
    if (objectBrowserRef && pendingPrefix) {
      objectBrowserRef.navigateTo(pendingPrefix);
      pendingPrefix = null;
    }
  });

  const isRootUser = $derived(
    sessionIsRoot ?? authQuery.data?.isRoot === true,
  );
  const canCreateBucket = $derived(
    isRootUser || authQuery.data?.capabilities?.canCreateBucket === true,
  );

  const sidebarNavItems = $derived(
    buildSidebarNavItems(
      { currentView, selectedBucket, isRootUser },
      { goHome, goUsers, goMetrics, goServerSettings },
    ).map((item) => ({
      ...item,
      onSelect: () => {
        item.onSelect();
        mobileMenuOpen = false;
      },
    })),
  );

  function closeMobileMenu() {
    mobileMenuOpen = false;
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && mobileMenuOpen) {
      mobileMenuOpen = false;
    }
  }

  $effect(() => {
    if (!mobileMenuOpen) return;
    document.body.classList.add("overflow-hidden");
    return () => document.body.classList.remove("overflow-hidden");
  });

  function toggleSidebarCollapsed() {
    collapsed = !collapsed;
    localStorage.setItem("sidebar-collapsed", String(collapsed));
  }

  $effect(() => {
    if (authQuery.data?.isRoot !== undefined) {
      sessionIsRoot = authQuery.data.isRoot === true;
    }
  });

  $effect(() => {
    const authResolved =
      authenticatedOverride !== null || authQuery.isSuccess || authQuery.isError;
    if (!authResolved) return;
    if (
      (currentView === "users" ||
        currentView === "metrics" ||
        currentView === "serverSettings") &&
      !isRootUser
    ) {
      goHome();
    }
  });

  function applyHash() {
    const hash = window.location.hash.slice(1) || "/";
    if (hash === "/") {
      selectedBucket = null;
      currentView = "objects";
      currentPrefix = "";
      currentBreadcrumbs = [];
    } else if (hash === "/users") {
      selectedBucket = null;
      currentView = "users";
      currentPrefix = "";
      currentBreadcrumbs = [];
    } else if (hash === "/metrics") {
      selectedBucket = null;
      currentView = "metrics";
      currentPrefix = "";
      currentBreadcrumbs = [];
    } else if (hash === "/settings") {
      selectedBucket = null;
      currentView = "serverSettings";
      currentPrefix = "";
      currentBreadcrumbs = [];
    } else {
      const parts = hash.slice(1).split("/"); // remove leading /
      const bucket = decodeURIComponent(parts[0]);
      const rest = parts.slice(1).join("/");
      selectedBucket = bucket;
      if (rest === "settings") {
        currentView = "settings";
        currentPrefix = "";
        currentBreadcrumbs = [];
      } else {
        currentView = "objects";
        if (rest) {
          if (objectBrowserRef) {
            objectBrowserRef.navigateTo(rest);
          } else {
            pendingPrefix = rest;
          }
        }
      }
    }
  }

  function updateHash() {
    if (!selectedBucket) {
      window.location.hash = "/";
    } else if (currentPrefix) {
      window.location.hash = `/${encodeURIComponent(selectedBucket)}/${currentPrefix}`;
    } else {
      window.location.hash = `/${encodeURIComponent(selectedBucket)}`;
    }
  }

  onMount(() => {
    collapsed = localStorage.getItem("sidebar-collapsed") === "true";
    const savedTheme = localStorage.getItem("theme");
    themeMode = isThemeMode(savedTheme) ? savedTheme : "system";
    applyTheme(themeMode, false);

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleSystemThemeChange = () => {
      if (themeMode === "system") {
        applyTheme("system", false);
      }
    };
    mediaQuery.addEventListener("change", handleSystemThemeChange);

    window.addEventListener("hashchange", applyHash);
    if (window.location.hash && window.location.hash !== "#/") {
      applyHash();
    }

    return () => {
      window.removeEventListener("hashchange", applyHash);
      mediaQuery.removeEventListener("change", handleSystemThemeChange);
    };
  });

  function handleLogin(session: AuthCheckResponse) {
    authenticatedOverride = true;
    sessionIsRoot = session.isRoot === true;
    queryClient.setQueryData(authKeys.check(), session);
    queryClient.invalidateQueries({ queryKey: authKeys.all });
  }

  async function handleLogout() {
    await logoutMutation.mutateAsync();
    authenticatedOverride = false;
    sessionIsRoot = null;
    selectedBucket = null;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
  }

  function applyTheme(mode: ThemeMode, persist = true) {
    themeMode = mode;
    isDark = applyThemeToDocument(mode);
    if (persist) {
      localStorage.setItem("theme", mode);
    }
  }

  function cycleTheme() {
    applyTheme(nextThemeMode(themeMode));
  }

  function selectBucket(name: string) {
    selectedBucket = name;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
    updateHash();
  }

  function goToSettings(name: string) {
    selectedBucket = name;
    currentView = "settings";
    currentPrefix = "";
    currentBreadcrumbs = [];
    window.location.hash = `/${encodeURIComponent(name)}/settings`;
  }

  function goHome() {
    selectedBucket = null;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
    updateHash();
  }

  function goUsers() {
    selectedBucket = null;
    currentView = "users";
    currentPrefix = "";
    currentBreadcrumbs = [];
    window.location.hash = "/users";
  }

  function goMetrics() {
    selectedBucket = null;
    currentView = "metrics";
    currentPrefix = "";
    currentBreadcrumbs = [];
    window.location.hash = "/metrics";
  }

  function goServerSettings() {
    selectedBucket = null;
    currentView = "serverSettings";
    currentPrefix = "";
    currentBreadcrumbs = [];
    window.location.hash = "/settings";
  }

  function handlePrefixChange(p: string, crumbs: { label: string; prefix: string }[]) {
    currentPrefix = p;
    currentBreadcrumbs = crumbs;
    updateHash();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if authQuery.isPending && authenticatedOverride === null}
  <!-- loading -->
{:else if !(authenticatedOverride ?? authQuery.isSuccess)}
  <Login onLogin={handleLogin} />
{:else}
  <div class="relative flex h-screen bg-background">
    {#if mobileMenuOpen}
      <button
        type="button"
        class="fixed inset-0 z-40 cursor-default bg-black/60 lg:hidden"
        aria-label="Close menu"
        onclick={closeMobileMenu}
        transition:fade={{ duration: 200 }}
      ></button>
      <div
        class="fixed inset-y-0 left-0 z-50 lg:hidden"
        transition:fly={{ x: -224, duration: 200 }}
      >
        <AppSidebar
          collapsed={false}
          variant="drawer"
          navItems={sidebarNavItems}
          username={authQuery.data?.username}
          isRoot={isRootUser}
          {themeMode}
          onToggleCollapsed={toggleSidebarCollapsed}
          onThemeChange={applyTheme}
          onCycleTheme={cycleTheme}
          onLogout={handleLogout}
        />
      </div>
    {/if}

    <AppSidebar
      {collapsed}
      variant="inline"
      class="hidden shrink-0 lg:flex"
      navItems={sidebarNavItems}
      username={authQuery.data?.username}
      isRoot={isRootUser}
      {themeMode}
      onToggleCollapsed={toggleSidebarCollapsed}
      onThemeChange={applyTheme}
      onCycleTheme={cycleTheme}
      onLogout={handleLogout}
    />

    <main class="flex min-w-0 flex-1 flex-col overflow-hidden">
      <header
        class="sticky top-0 z-30 flex h-14 shrink-0 items-center gap-3 border-b bg-background/95 px-4 backdrop-blur-sm lg:hidden"
        style="border-color: var(--cool-sidebar-border);"
      >
        <button
          type="button"
          onclick={() => (mobileMenuOpen = true)}
          class="-m-2.5 rounded-sm p-2.5 text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-warning dark:hover:text-warning dark:focus-visible:ring-warning"
          aria-label="Open menu"
          aria-expanded={mobileMenuOpen}
        >
          <Menu class="size-5" />
        </button>
        <img src={`${base}/maxio.png`} alt="" class="size-6 shrink-0" aria-hidden="true" />
        <span class="text-xl font-bold tracking-tight text-foreground">MaxIO</span>
      </header>

      {#if selectedBucket}
        <div
          class="flex min-h-14 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b px-6 py-2"
          style="border-color: var(--cool-sidebar-border);"
        >
          <button
            type="button"
            onclick={() => {
              if (currentView === "settings") {
                goHome();
              } else {
                objectBrowserRef?.goUp();
              }
            }}
            class="shrink-0 rounded-sm p-1 text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
            aria-label={currentView === "settings" ? "Back to buckets" : "Go up one folder"}
          >
            <ArrowLeft class="size-4" />
          </button>
          <nav aria-label="Breadcrumb" class="min-w-0 overflow-x-auto">
            <ol class="flex flex-wrap items-center gap-1.5 text-sm font-medium">
              <li class="inline-flex items-center gap-1.5">
                <button
                  type="button"
                  class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                  onclick={goHome}>Buckets</button
                >
              </li>
              <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                <ChevronRight class="size-3 shrink-0" />
              </li>
              {#if currentView === "settings"}
                <li class="inline-flex items-center gap-1.5">
                  <button
                    type="button"
                    class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                    onclick={() => selectBucket(selectedBucket!)}
                  >{selectedBucket}</button>
                </li>
                <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                  <ChevronRight class="size-3 shrink-0" />
                </li>
                <li class="inline-flex items-center gap-1.5">
                  <span class="shrink-0 text-black dark:text-white" aria-current="page">Settings</span>
                </li>
              {:else if currentBreadcrumbs.length > 1}
                {#each currentBreadcrumbs as crumb, i}
                  {#if i < currentBreadcrumbs.length - 1}
                    <li class="inline-flex items-center gap-1.5">
                      <button
                        type="button"
                        class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                        onclick={() => objectBrowserRef?.navigateTo(crumb.prefix)}
                      >{crumb.label}</button>
                    </li>
                    <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                      <ChevronRight class="size-3 shrink-0" />
                    </li>
                  {:else}
                    <li class="inline-flex items-center gap-1.5">
                      <span class="shrink-0 text-black dark:text-white" aria-current="page">{crumb.label}</span>
                    </li>
                  {/if}
                {/each}
              {:else}
                <li class="inline-flex items-center gap-1.5">
                  <span class="shrink-0 text-black dark:text-white" aria-current="page">{selectedBucket}</span>
                </li>
              {/if}
            </ol>
          </nav>
          {#if currentView === "objects" && bucketStats && (bucketStats.objectCount !== null || bucketStats.sizeBytes !== null)}
            <div
              class="flex w-full shrink-0 items-center gap-2 pl-6 text-sm text-muted-foreground sm:ml-auto sm:w-auto sm:pl-0"
              aria-label="Bucket statistics"
            >
              {#if bucketStats.objectCount !== null}
                <span class="tabular-nums">
                  <span class="font-medium text-foreground">{bucketStats.objectCount.toLocaleString()}</span>
                  {bucketStats.objectCount === 1 ? "object" : "objects"}
                </span>
              {/if}
              {#if bucketStats.objectCount !== null && bucketStats.sizeBytes !== null}
                <span class="text-neutral-300 dark:text-neutral-600" aria-hidden="true">·</span>
              {/if}
              {#if bucketStats.sizeBytes !== null}
                <span class="font-medium tabular-nums text-foreground">{formatBytes(bucketStats.sizeBytes)}</span>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
      <div class="flex-1 overflow-auto p-6">
        {#if currentView === "users" && isRootUser}
          <UserList />
        {:else if currentView === "metrics" && isRootUser}
          <Metrics />
        {:else if currentView === "serverSettings" && isRootUser}
          <ServerSettings />
        {:else if selectedBucket && currentView === "settings"}
          <BucketSettings
            bucket={selectedBucket}
            onBack={() => selectBucket(selectedBucket!)}
          />
        {:else if selectedBucket}
          <ObjectBrowser
            bind:this={objectBrowserRef}
            bucket={selectedBucket}
            onBack={goHome}
            onPrefixChange={handlePrefixChange}
          />
        {:else}
          <BucketList
            onSelect={selectBucket}
            onSettings={goToSettings}
            canCreateBucket={canCreateBucket}
          />
        {/if}
      </div>
    </main>
  </div>
  <Sonner theme={isDark ? 'dark' : 'light'} />
{/if}
