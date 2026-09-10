/**
 * Content script shell. Measurement, the scroll plan and sticky-element hiding
 * arrive in task 7, and the message handling that drives them in task 8.
 *
 * This file is injected on demand by `chrome.scripting`, never declared in the
 * manifest, so nothing runs on a page the user did not point at. It imports
 * nothing from `@snapdeck/protocol` and must keep it that way: the bridge is
 * the service worker's business, and the less this file carries the less rides
 * along into someone's page.
 */
export {}
