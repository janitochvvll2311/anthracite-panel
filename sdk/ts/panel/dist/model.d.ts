export type FabAction = {
    label: string;
    icon?: string;
    icon_sha256?: string;
    tag: string;
    target?: string;
    action?: string;
    files?: boolean;
};
export type Slot = {
    id: string;
    label: string;
    icon?: string;
    icon_sha256?: string;
    order: number;
    hidden?: boolean;
    parent?: string;
    tab?: string;
    view?: string;
    actions?: FabAction[];
    tag: string;
    module: string;
    sha256: string;
};
export type App = {
    id: string;
    version: string;
    risk: string;
    icon?: string;
    icon_sha256?: string;
    slots: Slot[];
};
export type Contribution = {
    app: App;
    slot: Slot;
};
