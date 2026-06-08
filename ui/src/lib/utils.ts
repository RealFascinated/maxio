import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

export function displayName(fullPath: string): string {
	const trimmed = fullPath.endsWith('/') ? fullPath.slice(0, -1) : fullPath
	const lastSlash = trimmed.lastIndexOf('/')
	return lastSlash >= 0 ? trimmed.slice(lastSlash + 1) : trimmed
}

export type { WithElementRef } from "bits-ui";
