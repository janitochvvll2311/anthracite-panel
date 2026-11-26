export interface AnthBuild {
    onResolve(options: {
        filter: RegExp;
    }, callback: (args: {
        path: string;
    }) => {
        path: string;
        namespace: string;
    } | undefined): void;
    onLoad(options: {
        filter: RegExp;
        namespace: string;
    }, callback: (args: {
        path: string;
    }) => {
        contents: string;
        loader: string;
    } | undefined): void;
}
export declare function anthPlugin(): {
    name: string;
    setup(build: AnthBuild): void;
};
export default anthPlugin;
