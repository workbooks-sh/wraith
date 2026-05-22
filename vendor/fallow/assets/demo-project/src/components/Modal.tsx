import React from "react";
import { Test } from "./Test";

export const Modal = ({ children }: { children: string }) => <Test>{children}</Test>;

export const useModal = () => ({ isOpen: false, toggle: () => {} });
