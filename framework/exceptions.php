<?php
/**
 * xhjob 框架异常体系
 *
 * 所有框架异常的基类与子类，供 XhjobService / TaskManager / Client / HttpApi 使用。
 */

/** 框架异常基类 */
class XhjobFrameworkException extends Exception {}

/** daemon 未运行时抛出 */
class ServiceNotRunningException extends XhjobFrameworkException {}

/** 任务未找到时抛出 */
class TaskNotFoundException extends XhjobFrameworkException {}

/** 任务配置无效时抛出 */
class InvalidTaskConfigException extends XhjobFrameworkException {}

/** IPC 通信错误时抛出 */
class IpcException extends XhjobFrameworkException {}
