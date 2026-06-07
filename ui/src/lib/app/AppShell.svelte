<script lang="ts">
  import { onMount } from "svelte";
  import { createMutation, createQuery } from "@tanstack/svelte-query";
  import Login from "$lib/Login.svelte";
  import BucketList from "$lib/BucketList.svelte";
  import ObjectBrowser from "$lib/ObjectBrowser.svelte";
  import BucketSettings from "$lib/BucketSettings.svelte";
  import UserList from "$lib/UserList.svelte";
  import AppSidebar from "$lib/app/sidebar/AppSidebar.svelte";
  import { buildSidebarNavItems } from "$lib/app/sidebar/navigation";
  import {
    applyThemeToDocument,
    isThemeMode,
    nextThemeMode,
    type ThemeMode,
  } from "$lib/app/sidebar/theme";
  import ArrowLeft from "lucide-svelte/icons/arrow-left";
  import ChevronRight from "lucide-svelte/icons/chevron-right";
  import { Sonner } from "$lib/components/ui/sonner";
  import { checkAuth, logout, type AuthCheckResponse } from "$lib/api/auth";
  import { authKeys } from "$lib/api/keys";
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
  let currentView = $state<"objects" | "settings" | "users">("objects");
  let objectBrowserRef = $state<ObjectBrowser | null>(null);
  let currentPrefix = $state("");
  let currentBreadcrumbs = $state<{ label: string; prefix: string }[]>([]);
  let themeMode = $state<ThemeMode>("system");
  let isDark = $state(true);
  let pendingPrefix = $state<string | null>(null);

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
      { goHome, goUsers },
    ),
  );

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
    if (currentView === "users" && !isRootUser) {
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

  function handlePrefixChange(p: string, crumbs: { label: string; prefix: string }[]) {
    currentPrefix = p;
    currentBreadcrumbs = crumbs;
    updateHash();
  }
</script>

{#if authQuery.isPending && authenticatedOverride === null}
  <!-- loading -->
{:else if !(authenticatedOverride ?? authQuery.isSuccess)}
  <Login onLogin={handleLogin} />
{:else}
  <div class="relative flex h-screen bg-background">
    <AppSidebar
      {collapsed}
      navItems={sidebarNavItems}
      {themeMode}
      onToggleCollapsed={toggleSidebarCollapsed}
      onThemeChange={applyTheme}
      onCycleTheme={cycleTheme}
      onLogout={handleLogout}
    />

    <main class="flex flex-1 flex-col overflow-hidden">
      {#if selectedBucket}
        <div class="flex h-14 shrink-0 items-center gap-2 px-6">
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
        </div>
      {/if}
      <div class="flex-1 overflow-auto p-6">
        {#if currentView === "users" && isRootUser}
          <UserList />
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
