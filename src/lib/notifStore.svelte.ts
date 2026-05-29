export type NotifKind = "info" | "warn" | "error" | "success";

export type Notif = {
	id: number;
	kind: NotifKind;
	title: string;
	body?: string;
	at: number;
	read: boolean;
};

let nextId = 1;

export const notifs = $state({
	list: [] as Notif[],
});

export function notify(kind: NotifKind, title: string, body?: string) {
	const dup = notifs.list.find(
		(n) => n.kind === kind && n.title === title && Date.now() - n.at < 5_000
	);
	if (dup) return;
	notifs.list.unshift({
		id: nextId++,
		kind,
		title,
		body,
		at: Date.now(),
		read: false,
	});
	if (notifs.list.length > 50) notifs.list = notifs.list.slice(0, 50);
}

export function markAllRead() {
	for (const n of notifs.list) n.read = true;
}

export function clearAll() {
	notifs.list = [];
}

export function unreadCount(): number {
	return notifs.list.filter((n) => !n.read).length;
}
