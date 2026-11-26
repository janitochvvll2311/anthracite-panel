export type CommandParam = {
    name: string;
    type: string;
    optional: boolean;
};
export type Command = {
    verb: string;
    method: string | null;
    path: string | null;
    interface: string;
    function: string;
    params: CommandParam[];
    returns: string;
};
export declare const commands: Command[];
